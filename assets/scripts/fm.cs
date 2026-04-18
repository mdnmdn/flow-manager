#!/usr/bin/dotnet run
#:package DotNetEnv@3.1.1
#:package System.CommandLine@2.0.0-beta4.22272.1
#:package YamlDotNet@16.3.0
#:property PublishAot=false
#:property PublishTrimmed=false

#nullable enable

using System.CommandLine;
using System.Diagnostics;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using DotNetEnv;
using YamlDotNet.Serialization;
using YamlDotNet.Serialization.NamingConventions;

var envPath = Path.Combine(Directory.GetCurrentDirectory(), ".env");
if (!File.Exists(envPath))
    envPath = Path.Combine(AppContext.BaseDirectory, ".env");
if (File.Exists(envPath))
    Env.Load(envPath);

var jsonOpts = new JsonSerializerOptions { WriteIndented = true };
var yaml = new SerializerBuilder()
    .WithNamingConvention(CamelCaseNamingConvention.Instance)
    .Build();

var defaultFormat = Environment.GetEnvironmentVariable("FM_FORMAT") ?? "markdown";
var defaultMergeStrategy = Environment.GetEnvironmentVariable("FM_MERGE_STRATEGY") ?? "squash";
var defaultTarget = Environment.GetEnvironmentVariable("FM_DEFAULT_TARGET") ?? "main";
var defaultWiType = Environment.GetEnvironmentVariable("FM_DEFAULT_WI_TYPE") ?? "User Story";
var docsSubmodule = Environment.GetEnvironmentVariable("FM_DOCS_SUBMODULE") ?? "_docs";

var scriptDir = Path.GetDirectoryName(Environment.ProcessPath ?? string.Empty)
    ?? Path.GetDirectoryName(AppContext.BaseDirectory)
    ?? ".";
var adoScript = Path.Combine(scriptDir, "ado.cs");
var gitScript = Path.Combine(scriptDir, "git.cs");
var todoScript = Path.Combine(scriptDir, "todo.cs");
var sonarScript = Path.Combine(scriptDir, "sonar.cs");

var formatOpt = new Option<string>("--format", () => defaultFormat, "Output format: markdown, json, yaml");

var workCmd = new Command("work", "Manage work activities");
var taskCmd = new Command("task", "Manage the current activity");
var prCmd = new Command("pr", "Manage pull requests");
var pipelineCmd = new Command("pipeline", "Manage pipelines");
var todoCmd = new Command("todo", "Manage child tasks");

// ── work new ────────────────────────────────────────────────────────────────

var workNewTitleOpt = new Option<string>("--title", "Work item title") { IsRequired = true };
var workNewDescOpt = new Option<string?>("--description", () => null, "Work item description");
var workNewBranchOpt = new Option<string?>("--branch", () => null, "Branch slug suffix");
var workNewTypeOpt = new Option<string>("--type", () => "feature", "feature or fix");
var workNewTargetOpt = new Option<string>("--target", () => defaultTarget, "Target baseline branch");
var workNewAssignedOpt = new Option<string?>("--assigned-to", () => null, "Assigned-to");
var workNewTagsOpt = new Option<string?>("--tags", () => null, "Semicolon-separated tags");
var workNewSonarOpt = new Option<string?>("--sonar-project", () => null, "SonarQube project key");

var workNewCmd = new Command("new", "Create a WI, branch, and draft PR, then switch locally")
{
    workNewTitleOpt, workNewDescOpt, workNewBranchOpt, workNewTypeOpt, workNewTargetOpt, workNewAssignedOpt, workNewTagsOpt, workNewSonarOpt, formatOpt
};
workNewCmd.SetHandler(async (string title, string? description, string? branchSuffix, string type, string target, string? assignedTo, string? tags, string? sonarProject, string format) =>
{
    var repo = ResolveRepo();
    var wiType = type.Equals("fix", StringComparison.OrdinalIgnoreCase) ? "Bug" : defaultWiType;
    var branchKind = type.Equals("fix", StringComparison.OrdinalIgnoreCase) ? "fix" : "feature";

    if (!string.IsNullOrWhiteSpace(sonarProject))
        description = await AppendSonarIssues(description, sonarProject);

    var existing = await FindExistingWorkItem(title, wiType);
    var wi = existing ?? await RunAdoJson("wi", "create", "--type", wiType, "--title", title, "--format", "json",
        description is null ? null : "--description",
        description,
        assignedTo is null ? null : "--assigned-to",
        assignedTo,
        tags is null ? null : "--tags",
        tags);

    var wiId = wi.GetProperty("id").GetInt32();
    var branch = $"{branchKind}/{wiId}-{Slugify(branchSuffix ?? title)}";

    var branchExists = await RemoteBranchExists(repo, branch);
    if (branchExists)
    {
        Render(format, new ErrorResult("Branch already exists", $"Remote branch `{branch}` already exists. Use `fm work load {wiId}` instead."));
        Environment.Exit(1);
        return;
    }

    await RunAdoJson("refs", "create", "--repo", repo, "--branch", branch, "--source", target, "--format", "json");
    var pr = await EnsureSingleActivePr(repo, branch, title, target, wiId, createIfMissing: true, draft: true);
    await RunAdoJson("wi", "link", "--id", wiId.ToString(), "--repo", repo, "--branch", branch, "--pr", pr.GetProperty("pullRequestId").GetInt32().ToString(), "--format", "json");
    await EnsureWiState(wiId, "Active");

    await RunGitJson("fetch", "--format", "json");
    await RunGitJson("checkout-remote", "--name", branch, "--format", "json");

    Render(format, new WorkActivityResult(
        wiId,
        title,
        wiType,
        "Active",
        branch,
        pr.GetProperty("pullRequestId").GetInt32(),
        pr.TryGetProperty("isDraft", out var isDraftEl) && isDraftEl.GetBoolean() ? "draft" : ExtractString(pr, "status"),
        target,
        "— (no commits yet)",
        null,
        "started"));
}, workNewTitleOpt, workNewDescOpt, workNewBranchOpt, workNewTypeOpt, workNewTargetOpt, workNewAssignedOpt, workNewTagsOpt, workNewSonarOpt, formatOpt);
workCmd.AddCommand(workNewCmd);

// ── work load ───────────────────────────────────────────────────────────────

var workLoadIdArg = new Argument<string>("id", "WI id, PR id, or branch name");
var workLoadTargetOpt = new Option<string>("--target", () => defaultTarget, "Target baseline branch");

var workLoadCmd = new Command("load", "Resume an existing work item")
{
    workLoadIdArg, workLoadTargetOpt, formatOpt
};
workLoadCmd.SetHandler(async (string rawId, string target, string format) =>
{
    var repo = ResolveRepo();
    var resolved = await ResolveId(repo, rawId);
    if (resolved.WiId is null)
    {
        Render(format, new ErrorResult("Work item not found", $"Cannot resolve `{rawId}` to a work item."));
        Environment.Exit(1);
        return;
    }

    var wi = await RunAdoJson("wi", "get", "--id", resolved.WiId.Value.ToString(), "--format", "json");
    var fields = wi.GetProperty("fields");
    var state = ExtractField(fields, "System.State");
    var title = ExtractField(fields, "System.Title");
    var wiType = ExtractField(fields, "System.WorkItemType");

    if (state is "Closed" or "Done" or "Removed")
    {
        Render(format, new WorkClosedResult(resolved.WiId.Value, title, state, TryExtractPrIdFromWi(wi)));
        return;
    }

    var branch = BranchNameForWorkItem(resolved.WiId.Value, title, wiType);
    if (!await RemoteBranchExists(repo, branch))
        await RunAdoJson("refs", "create", "--repo", repo, "--branch", branch, "--source", target, "--format", "json");

    var pr = await EnsureSingleActivePr(repo, branch, title, target, resolved.WiId.Value, createIfMissing: true, draft: true);
    await RunAdoJson("wi", "link", "--id", resolved.WiId.Value.ToString(), "--repo", repo, "--branch", branch, "--pr", pr.GetProperty("pullRequestId").GetInt32().ToString(), "--format", "json");
    await EnsureWiState(resolved.WiId.Value, "Active");

    await RunGitJson("fetch", "--format", "json");
    await RunGitJson("checkout-remote", "--name", branch, "--format", "json");

    var stashRestored = await RestoreNamedStash($"stash-{resolved.WiId.Value}-");
    Render(format, new WorkActivityResult(
        resolved.WiId.Value,
        title,
        wiType,
        "Active",
        branch,
        pr.GetProperty("pullRequestId").GetInt32(),
        pr.TryGetProperty("isDraft", out var isDraftEl) && isDraftEl.GetBoolean() ? "draft" : ExtractString(pr, "status"),
        target,
        "—",
        stashRestored,
        "loaded"));
}, workLoadIdArg, workLoadTargetOpt, formatOpt);
workCmd.AddCommand(workLoadCmd);

// ── work list ───────────────────────────────────────────────────────────────

var workListMineOpt = new Option<bool>("--mine", "Filter by current user");
var workListStateOpt = new Option<string>("--state", () => "Active", "State filter");
var workListTypeOpt = new Option<string>("--type", () => "all", "feature, fix, all");
var workListMaxOpt = new Option<int>("--max", () => 20, "Maximum results");

var workListCmd = new Command("list", "List work items")
{
    workListMineOpt, workListStateOpt, workListTypeOpt, workListMaxOpt, formatOpt
};
workListCmd.SetHandler(async (bool mine, string state, string type, int max, string format) =>
{
    var project = Environment.GetEnvironmentVariable("ADO_PROJECT")
        ?? throw new InvalidOperationException("ADO_PROJECT environment variable is not set.");
    var query = new StringBuilder($"SELECT [System.Id] FROM WorkItems WHERE [System.TeamProject] = '{project}'");
    if (!string.Equals(state, "all", StringComparison.OrdinalIgnoreCase))
        query.Append($" AND [System.State] = '{state.Replace("'", "''")}'");

    if (type.Equals("feature", StringComparison.OrdinalIgnoreCase))
        query.Append($" AND [System.WorkItemType] = '{defaultWiType.Replace("'", "''")}'");
    else if (type.Equals("fix", StringComparison.OrdinalIgnoreCase))
        query.Append(" AND [System.WorkItemType] = 'Bug'");
    else
        query.Append($" AND ([System.WorkItemType] = '{defaultWiType.Replace("'", "''")}' OR [System.WorkItemType] = 'Bug')");

    if (mine)
        query.Append(" AND [System.AssignedTo] = @Me");
    query.Append(" ORDER BY [System.ChangedDate] DESC");

    var result = await RunAdoJson("wi", "search", "--query", query.ToString(), "--max-results", max.ToString(), "--format", "json");
    Render(format, new WorkListResult(result.GetProperty("count").GetInt32(), result.GetProperty("items").EnumerateArray().Select(x => x.Clone()).ToList()));
}, workListMineOpt, workListStateOpt, workListTypeOpt, workListMaxOpt, formatOpt);
workCmd.AddCommand(workListCmd);

// ── task hold/update/complete/sync ──────────────────────────────────────────

var holdStashOpt = new Option<bool>("--stash", "Stash uncommitted changes");
var holdForceOpt = new Option<bool>("--force", "Discard uncommitted changes");
var holdStayOpt = new Option<bool>("--stay", "Stay on the current branch");
var taskHoldCmd = new Command("hold", "Pause the current activity")
{
    holdStashOpt, holdForceOpt, holdStayOpt, formatOpt
};
taskHoldCmd.SetHandler(async (bool stash, bool force, bool stay, string format) =>
{
    var ctx = ParseBranchContext();
    if (!ctx.IsActivity)
    {
        Render(format, new TaskHoldResult(ctx.BranchName, null, null, "Already on baseline — nothing to hold."));
        return;
    }

    var status = await RunGitJson("status", "--porcelain", "--format", "json");
    var clean = status.GetProperty("clean").GetBoolean();
    string? stashName = null;

    if (!clean && !stash && !force)
    {
        Render(format, new ErrorResult("Hold blocked",
            $"Uncommitted changes present.\n\n```\n{status.GetProperty("status").GetString()?.TrimEnd()}\n```\n\nUse `--stash` to save them or `--force` to discard."));
        Environment.Exit(1);
        return;
    }

    if (!clean && stash)
    {
        stashName = $"stash-{ctx.WorkItemId}-{ctx.Slug}";
        await RunGitJson("stash-save", "--message", stashName, "--format", "json");
    }

    if (!clean && force)
        await RunGitJson("discard", "--format", "json");

    await RunGitJson("push", "--format", "json");

    var nowOn = ctx.BranchName;
    if (!stay)
    {
        await RunGitJson("checkout", "--name", defaultTarget, "--format", "json");
        nowOn = defaultTarget;
    }

    Render(format, new TaskHoldResult(ctx.BranchName, stashName, nowOn, null));
}, holdStashOpt, holdForceOpt, holdStayOpt, formatOpt);
taskCmd.AddCommand(taskHoldCmd);

var taskUpdateTitleOpt = new Option<string?>("--title", () => null, "New title");
var taskUpdateStateOpt = new Option<string?>("--state", () => null, "New state");
var taskUpdateDescOpt = new Option<string?>("--description", () => null, "New description");
var taskUpdateAssignedOpt = new Option<string?>("--assigned-to", () => null, "New assigned-to");
var taskUpdateTagsOpt = new Option<string?>("--tags", () => null, "New tags");
var taskUpdateCmd = new Command("update", "Update the work item linked to the current activity")
{
    taskUpdateTitleOpt, taskUpdateStateOpt, taskUpdateDescOpt, taskUpdateAssignedOpt, taskUpdateTagsOpt, formatOpt
};
taskUpdateCmd.SetHandler(async (string? title, string? state, string? description, string? assignedTo, string? tags, string format) =>
{
    var ctx = EnsureActivity(ParseBranchContext());
    var repo = ResolveRepo();
    await EnsureActivityInvariants(repo, ctx);

    var wi = await RunAdoJson("wi", "update", "--id", ctx.WorkItemId!.Value.ToString(), "--format", "json",
        title is null ? null : "--title", title,
        state is null ? null : "--state", state,
        description is null ? null : "--description", description,
        assignedTo is null ? null : "--assigned-to", assignedTo,
        tags is null ? null : "--tags", tags);

    Render(format, new TaskUpdateResult(ctx.WorkItemId.Value, wi));
}, taskUpdateTitleOpt, taskUpdateStateOpt, taskUpdateDescOpt, taskUpdateAssignedOpt, taskUpdateTagsOpt, formatOpt);
taskCmd.AddCommand(taskUpdateCmd);

var taskCompleteCmd = new Command("complete", "Return to baseline after the activity is done") { formatOpt };
taskCompleteCmd.SetHandler(async (string format) =>
{
    var ctx = ParseBranchContext();
    if (!ctx.IsActivity)
    {
        Render(format, new ErrorResult("Already on baseline", "Nothing to complete."));
        Environment.Exit(1);
        return;
    }

    var repo = ResolveRepo();
    var wiTask = RunAdoJson("wi", "get", "--id", ctx.WorkItemId!.Value.ToString(), "--format", "json");
    var prsTask = RunAdoJson("pr", "list", "--repo", repo, "--source", ctx.BranchName, "--status", "all", "--format", "json");
    await Task.WhenAll(wiTask, prsTask);

    var wi = await wiTask;
    var prList = await prsTask;
    var wiState = ExtractField(wi.GetProperty("fields"), "System.State");
    var prs = prList.GetProperty("prs").EnumerateArray().Select(x => x.Clone()).ToList();
    var pr = prs.FirstOrDefault();
    if (pr.ValueKind != JsonValueKind.Object)
    {
        Render(format, new ErrorResult("Cannot complete", $"No PR found for branch `{ctx.BranchName}`."));
        Environment.Exit(1);
        return;
    }

    var prState = ExtractString(pr, "status");
    var isDraft = pr.TryGetProperty("isDraft", out var draftEl) && draftEl.GetBoolean();
    var prId = pr.GetProperty("pullRequestId").GetInt32();
    if (prState == "active" || isDraft)
    {
        Render(format, new ErrorResult("Cannot complete",
            $"PR #{prId} is `{(isDraft ? "draft" : prState)}`. Merge or publish it first."));
        Environment.Exit(1);
        return;
    }

    await RunGitJson("checkout", "--name", defaultTarget, "--format", "json");
    await RunGitJson("pull", "--format", "json");

    Render(format, new TaskCompleteResult(ctx.WorkItemId.Value, wiState, prId, prState, defaultTarget));
}, formatOpt);
taskCmd.AddCommand(taskCompleteCmd);

var syncRebaseOpt = new Option<bool>("--rebase", "Use rebase instead of merge");
var syncCheckOpt = new Option<bool>("--check", "Dry-run only");
var taskSyncCmd = new Command("sync", "Sync the activity branch with the baseline")
{
    syncRebaseOpt, syncCheckOpt, formatOpt
};
taskSyncCmd.SetHandler(async (bool rebase, bool check, string format) =>
{
    var ctx = EnsureActivity(ParseBranchContext());
    var repo = ResolveRepo();
    await EnsureActivityInvariants(repo, ctx);

    await RunGitJson("fetch", "--format", "json");
    var diff = await RunGitJson("diff", "--target", $"origin/{defaultTarget}", "--format", "json");
    var ahead = diff.GetProperty("ahead").GetInt32();
    var behind = diff.GetProperty("behind").GetInt32();
    var behindCommits = diff.GetProperty("commitsBehind").EnumerateArray().Select(x => x.GetString() ?? string.Empty).ToList();
    var aheadCommits = diff.GetProperty("commitsAhead").EnumerateArray().Select(x => x.GetString() ?? string.Empty).ToList();

    if (check)
    {
        Render(format, new TaskSyncCheckResult(ctx.BranchName, defaultTarget, ahead, behind, behindCommits, aheadCommits));
        return;
    }

    if (behind == 0)
    {
        Render(format, new TaskSyncResult(ctx.BranchName, rebase ? "rebase" : "merge", defaultTarget, behind, true, new List<string>()));
        return;
    }

    var integration = rebase
        ? await RunJsonAllowFailure(gitScript, "rebase", "--target", $"origin/{defaultTarget}", "--format", "json")
        : await RunJsonAllowFailure(gitScript, "merge", "--target", $"origin/{defaultTarget}", "--format", "json");

    var success = integration.GetProperty("success").GetBoolean();
    if (!success)
    {
        var conflicts = integration.GetProperty("conflicts").EnumerateArray().Select(x => x.GetString() ?? string.Empty).ToList();
        Render(format, new TaskSyncConflictResult(ctx.BranchName, rebase ? "rebase" : "merge", defaultTarget, conflicts));
        Environment.Exit(1);
        return;
    }

    await RunGitJson("push", "--format", "json");
    Render(format, new TaskSyncResult(ctx.BranchName, rebase ? "rebase" : "merge", defaultTarget, behind, true, behindCommits));
}, syncRebaseOpt, syncCheckOpt, formatOpt);
taskCmd.AddCommand(taskSyncCmd);

// ── pr show/update/merge/review ─────────────────────────────────────────────

var prShowIdArg = new Argument<string?>("id", () => null, "PR id, WI id, or branch");
var prShowCmd = new Command("show", "Show PR details")
{
    prShowIdArg, formatOpt
};
prShowCmd.SetHandler(async (string? rawId, string format) =>
{
    var repo = ResolveRepo();
    int? prId = null;

    if (!string.IsNullOrWhiteSpace(rawId))
    {
        var resolved = await ResolveId(repo, rawId);
        prId = resolved.PrId;
        if (!prId.HasValue && resolved.WiId.HasValue)
            prId = await ResolvePrIdFromWorkItem(repo, resolved.WiId.Value);
    }
    else
    {
        var ctx = EnsureActivity(ParseBranchContext());
        var pr = await EnsureSingleActivePr(repo, ctx.BranchName, null, defaultTarget, ctx.WorkItemId!.Value, createIfMissing: false, draft: true);
        prId = pr.GetProperty("pullRequestId").GetInt32();
    }

    if (!prId.HasValue)
    {
        Render(format, new ErrorResult("PR not found", "No PR matched the given identifier."));
        Environment.Exit(1);
        return;
    }

    var pr = await RunAdoJson("pr", "get", "--repo", repo, "--id", prId.Value.ToString(), "--format", "json");
    Render(format, new PrShowResult(prId.Value, pr.GetProperty("pr")));
}, prShowIdArg, formatOpt);
prCmd.AddCommand(prShowCmd);

var prUpdateTitleOpt = new Option<string?>("--title", () => null, "New title");
var prUpdateDescOpt = new Option<string?>("--description", () => null, "New description");
var prUpdatePublishOpt = new Option<bool>("--publish", "Publish the draft PR");
var prUpdateStatusOpt = new Option<string?>("--status", () => null, "active, abandoned, completed");
var prUpdateReviewerOpt = new Option<string[]>("--add-reviewer", "Reviewer email/unique name") { AllowMultipleArgumentsPerToken = true };
var prUpdateCmd = new Command("update", "Update the PR linked to the current activity")
{
    prUpdateTitleOpt, prUpdateDescOpt, prUpdatePublishOpt, prUpdateStatusOpt, prUpdateReviewerOpt, formatOpt
};
prUpdateCmd.SetHandler(async (string? title, string? description, bool publish, string? status, string[] reviewers, string format) =>
{
    var ctx = EnsureActivity(ParseBranchContext());
    var repo = ResolveRepo();
    await EnsureActivityInvariants(repo, ctx);
    var pr = await EnsureSingleActivePr(repo, ctx.BranchName, null, defaultTarget, ctx.WorkItemId!.Value, createIfMissing: false, draft: true);
    var prId = pr.GetProperty("pullRequestId").GetInt32();

    var updateArgs = new List<string?> { "pr", "update", "--repo", repo, "--id", prId.ToString(), "--format", "json" };
    if (title is not null) { updateArgs.Add("--title"); updateArgs.Add(title); }
    if (description is not null) { updateArgs.Add("--description"); updateArgs.Add(description); }
    if (status is not null) { updateArgs.Add("--status"); updateArgs.Add(status); }
    if (publish) updateArgs.Add("--publish");
    foreach (var reviewer in reviewers.Where(x => !string.IsNullOrWhiteSpace(x)))
    {
        updateArgs.Add("--add-reviewer");
        updateArgs.Add(reviewer);
    }

    var updated = await RunAdoJson(updateArgs.ToArray());

    Render(format, new PrShowResult(prId, updated.GetProperty("pr")));
}, prUpdateTitleOpt, prUpdateDescOpt, prUpdatePublishOpt, prUpdateStatusOpt, prUpdateReviewerOpt, formatOpt);
prCmd.AddCommand(prUpdateCmd);

var prMergeStrategyOpt = new Option<string?>("--strategy", () => null, "Merge strategy");
var prMergeDeleteOpt = new Option<bool>("--delete-source-branch", "Delete source branch after merge");
var prMergeBypassOpt = new Option<bool>("--bypass-policy", "Bypass branch policies");
var prMergeCmd = new Command("merge", "Complete the PR linked to the current activity")
{
    prMergeStrategyOpt, prMergeDeleteOpt, prMergeBypassOpt, formatOpt
};
prMergeCmd.SetHandler(async (string? strategy, bool deleteSource, bool bypass, string format) =>
{
    var ctx = EnsureActivity(ParseBranchContext());
    var repo = ResolveRepo();
    await EnsureActivityInvariants(repo, ctx);

    var pr = await EnsureSingleActivePr(repo, ctx.BranchName, null, defaultTarget, ctx.WorkItemId!.Value, createIfMissing: false, draft: true);
    var prId = pr.GetProperty("pullRequestId").GetInt32();
    var fullPr = await RunAdoJson("pr", "get", "--repo", repo, "--id", prId.ToString(), "--format", "json");
    var prRoot = fullPr.GetProperty("pr");

    if (prRoot.TryGetProperty("isDraft", out var draftEl) && draftEl.GetBoolean())
    {
        Render(format, new ErrorResult("PR is draft", $"PR #{prId} is draft. Run `fm pr update --publish` first."));
        Environment.Exit(1);
        return;
    }

    var mergeStatus = ExtractString(prRoot, "mergeStatus");
    if (!string.IsNullOrWhiteSpace(mergeStatus) &&
        !mergeStatus.Equals("succeeded", StringComparison.OrdinalIgnoreCase) &&
        !mergeStatus.Equals("queued", StringComparison.OrdinalIgnoreCase))
    {
        Render(format, new PrMergeErrorResult(prId, new List<string> { $"merge status: {mergeStatus}" }));
        Environment.Exit(1);
        return;
    }

    var merged = await RunAdoJson("pr", "merge", "--repo", repo, "--id", prId.ToString(), "--strategy", strategy ?? defaultMergeStrategy, "--format", "json",
        deleteSource ? "--delete-source-branch" : null,
        bypass ? "--bypass-policy" : null);
    await EnsureWiState(ctx.WorkItemId.Value, "Closed");

    var mergedPr = merged.GetProperty("pr");
    var commit = mergedPr.TryGetProperty("lastMergeCommit", out var lastMergeCommit) &&
                 lastMergeCommit.TryGetProperty("commitId", out var commitId)
        ? (commitId.GetString() ?? "—")
        : "—";
    var target = StripRefsHeads(ExtractString(mergedPr, "targetRefName"));

    Render(format, new PrMergeResult(
        prId,
        strategy ?? defaultMergeStrategy,
        ctx.WorkItemId.Value,
        target,
        commit.Length > 7 ? commit[..7] : commit));
}, prMergeStrategyOpt, prMergeDeleteOpt, prMergeBypassOpt, formatOpt);
prCmd.AddCommand(prMergeCmd);

var prReviewIdArg = new Argument<string>("id", "PR id, WI id, or branch");
var prReviewCmd = new Command("review", "Switch to another PR branch for review")
{
    prReviewIdArg, formatOpt
};
prReviewCmd.SetHandler(async (string rawId, string format) =>
{
    var repo = ResolveRepo();
    var current = ParseBranchContext();
    string? stashName = null;

    if (current.IsActivity)
    {
        var status = await RunGitJson("status", "--porcelain", "--format", "json");
        if (!status.GetProperty("clean").GetBoolean())
        {
            stashName = $"stash-{current.WorkItemId}-{current.Slug}";
            await RunGitJson("stash-save", "--message", stashName, "--format", "json");
        }

        await RunGitJson("push", "--format", "json");
    }

    var resolved = await ResolveId(repo, rawId);
    var prId = resolved.PrId ?? (resolved.WiId.HasValue ? await ResolvePrIdFromWorkItem(repo, resolved.WiId.Value) : null);
    if (!prId.HasValue)
    {
        Render(format, new ErrorResult("PR not found", $"Cannot resolve `{rawId}` to a PR."));
        Environment.Exit(1);
        return;
    }

    var pr = await RunAdoJson("pr", "get", "--repo", repo, "--id", prId.Value.ToString(), "--format", "json");
    var sourceBranch = StripRefsHeads(ExtractString(pr.GetProperty("pr"), "sourceRefName"));
    await RunGitJson("fetch", "--format", "json");
    await RunGitJson("checkout-remote", "--name", sourceBranch, "--format", "json");

    Render(format, new PrReviewResult(prId.Value, sourceBranch, current.IsActivity ? current.BranchName : null, stashName));
}, prReviewIdArg, formatOpt);
prCmd.AddCommand(prReviewCmd);

// ── pipeline run/status ─────────────────────────────────────────────────────

var pipelineRunIdOpt = new Option<int?>("--id", () => null, "Pipeline definition ID");
var pipelineRunCmd = new Command("run", "Run a pipeline for the current branch")
{
    pipelineRunIdOpt, formatOpt
};
pipelineRunCmd.SetHandler(async (int? id, string format) =>
{
    var branch = ParseBranchContext().BranchName;
    if (!id.HasValue)
    {
        var pipelines = await RunAdoJson("pipeline", "list", "--format", "json");
        Render(format, new PipelineListResult(
            pipelines.GetProperty("count").GetInt32(),
            pipelines.GetProperty("pipelines").EnumerateArray().Select(x => x.Clone()).ToList(),
            "--id required — pick one and re-run"));
        Environment.Exit(1);
        return;
    }

    var run = await RunAdoJson("pipeline", "run", "--id", id.Value.ToString(), "--branch", branch, "--format", "json");
    Render(format, new PipelineRunSummaryResult(id.Value, run.GetProperty("runId").GetInt32(), branch, run.GetProperty("run")));
}, pipelineRunIdOpt, formatOpt);
pipelineCmd.AddCommand(pipelineRunCmd);

var pipelineStatusRunIdOpt = new Option<int?>("--run-id", () => null, "Run ID");
var pipelineStatusWatchOpt = new Option<bool>("--watch", "Poll until completed");
var pipelineStatusCmd = new Command("status", "Show pipeline status for the current branch")
{
    pipelineStatusRunIdOpt, pipelineStatusWatchOpt, formatOpt
};
pipelineStatusCmd.SetHandler(async (int? runId, bool watch, string format) =>
{
    var branch = ParseBranchContext().BranchName;

    while (true)
    {
        JsonElement run;
        if (runId.HasValue)
            run = await RunAdoJson("pipeline", "get", "--run-id", runId.Value.ToString(), "--format", "json");
        else
            run = await RunAdoJson("pipeline", "latest", "--branch", branch, "--format", "json");

        JsonElement runPayload;
        int actualRunId;
        if (run.TryGetProperty("run", out var nestedRun))
        {
            runPayload = nestedRun;
            actualRunId = run.GetProperty("runId").GetInt32();
        }
        else
        {
            runPayload = run.GetProperty("runs")[0];
            actualRunId = runPayload.GetProperty("id").GetInt32();
        }

        var status = ExtractString(runPayload, "status");
        var result = runPayload.TryGetProperty("result", out var resultEl) ? resultEl.GetString() : null;
        var pipelineName = runPayload.TryGetProperty("definition", out var def) && def.TryGetProperty("name", out var nameEl)
            ? nameEl.GetString()
            : null;

        Render(format, new PipelineStatusResult(actualRunId, pipelineName, status, result, branch, runPayload));
        if (!watch || string.Equals(status, "completed", StringComparison.OrdinalIgnoreCase))
            return;

        await Task.Delay(TimeSpan.FromSeconds(30));
    }
}, pipelineStatusRunIdOpt, pipelineStatusWatchOpt, formatOpt);
pipelineCmd.AddCommand(pipelineStatusCmd);

// ── todo wrappers ───────────────────────────────────────────────────────────

todoCmd.AddCommand(BuildTodoShowCommand());
todoCmd.AddCommand(BuildTodoNewCommand());
todoCmd.AddCommand(BuildTodoRefCommand("pick", "Set a todo Active"));
todoCmd.AddCommand(BuildTodoRefCommand("complete", "Set a todo Closed"));
todoCmd.AddCommand(BuildTodoRefCommand("reopen", "Set a todo back to New"));
todoCmd.AddCommand(BuildTodoUpdateCommand());
todoCmd.AddCommand(BuildTodoNextCommand());

// ── context ─────────────────────────────────────────────────────────────────

var ctxOnlyWiOpt = new Option<bool>("--only-wi", "Show only work item details");
var ctxOnlyPrOpt = new Option<bool>("--only-pr", "Show only PR details");
var ctxOnlyGitOpt = new Option<bool>("--only-git", "Show only git details");
var ctxOnlyPipelineOpt = new Option<bool>("--only-pipeline", "Show only pipeline details");
var contextCmd = new Command("context", "Show the current workflow context")
{
    ctxOnlyWiOpt, ctxOnlyPrOpt, ctxOnlyGitOpt, ctxOnlyPipelineOpt, formatOpt
};
contextCmd.SetHandler(async (bool onlyWi, bool onlyPr, bool onlyGit, bool onlyPipeline, string format) =>
{
    var ctx = ParseBranchContext();
    if (!ctx.IsActivity)
    {
        var log = await RunGitJson("log", "--oneline", "--max-count", "5", "--format", "json");
        var commits = log.GetProperty("commits").EnumerateArray().Select(x => x.GetString() ?? string.Empty).ToList();
        Render(format, new ContextBaselineResult(ctx.BranchName, commits));
        return;
    }

    var repo = ResolveRepo();
    await EnsureActivityInvariants(repo, ctx);

    JsonElement? wi = null;
    JsonElement? pr = null;
    int? prId = null;
    JsonElement? pipeline = null;
    string? pipelineName = null;
    int? pipelineId = null;
    int ahead = 0;
    int behind = 0;
    bool clean = true;
    JsonElement? todoSnapshot = null;

    var tasks = new List<Task>();
    if (!onlyPr && !onlyGit && !onlyPipeline)
    {
        tasks.Add(Task.Run(async () =>
        {
            wi = await RunAdoJson("wi", "get", "--id", ctx.WorkItemId!.Value.ToString(), "--format", "json");
        }));
    }
    if (!onlyWi && !onlyGit && !onlyPipeline)
    {
        tasks.Add(Task.Run(async () =>
        {
            var prList = await RunAdoJson("pr", "list", "--repo", repo, "--source", ctx.BranchName, "--status", "active", "--format", "json");
            var prs = prList.GetProperty("prs").EnumerateArray().Select(x => x.Clone()).ToList();
            if (prs.Count > 0)
            {
                pr = prs[0];
                prId = pr.Value.GetProperty("pullRequestId").GetInt32();
            }
        }));
    }
    if (!onlyWi && !onlyPr && !onlyGit)
    {
        tasks.Add(Task.Run(async () =>
        {
            var latest = await RunAdoJson("pipeline", "latest", "--branch", ctx.BranchName, "--format", "json");
            if (latest.TryGetProperty("run", out var run))
            {
                pipeline = run;
                if (run.TryGetProperty("definition", out var def))
                {
                    pipelineName = def.TryGetProperty("name", out var nameEl) ? nameEl.GetString() : null;
                    pipelineId = def.TryGetProperty("id", out var idEl) ? idEl.GetInt32() : null;
                }
            }
        }));
    }
    if (!onlyWi && !onlyPr && !onlyGit && !onlyPipeline)
    {
        tasks.Add(Task.Run(async () =>
        {
            todoSnapshot = await RunTodoJson("show", "--wi-id", ctx.WorkItemId!.Value.ToString(), "--all", "--format", "json");
        }));
    }

    await RunGitJson("fetch", "--format", "json");
    if (!onlyWi && !onlyPr && !onlyPipeline)
    {
        var diffTask = RunGitJson("diff", "--target", $"origin/{defaultTarget}", "--format", "json");
        var statusTask = RunGitJson("status", "--porcelain", "--format", "json");
        await Task.WhenAll(diffTask, statusTask);

        var diff = await diffTask;
        ahead = diff.GetProperty("ahead").GetInt32();
        behind = diff.GetProperty("behind").GetInt32();
        clean = statusTask.Result.GetProperty("clean").GetBoolean();
    }

    if (tasks.Count > 0)
        await Task.WhenAll(tasks);

    Render(format, new ContextActivityResult(
        ctx.BranchName,
        wi,
        prId,
        pr,
        ahead,
        behind,
        clean,
        pipelineName,
        pipelineId,
        pipeline,
        todoSnapshot,
        onlyWi,
        onlyPr,
        onlyGit,
        onlyPipeline,
        defaultTarget));
}, ctxOnlyWiOpt, ctxOnlyPrOpt, ctxOnlyGitOpt, ctxOnlyPipelineOpt, formatOpt);

// ── commit/push/sync/sonar ─────────────────────────────────────────────────

var commitMessageOpt = new Option<string?>("--message", () => null, "Commit message");
var commitAllOpt = new Option<bool>("--all", "Stage tracked changes before commit");
var commitAmendOpt = new Option<bool>("--amend", "Amend the previous commit");
var commitDocsMessageOpt = new Option<string?>("--docs-message", () => null, "Override docs submodule commit message");
var commitNoDocsOpt = new Option<bool>("--no-docs", "Skip docs submodule handling");
var commitCmd = new Command("commit", "Commit changes, handling the docs submodule transparently")
{
    commitMessageOpt, commitAllOpt, commitAmendOpt, commitDocsMessageOpt, commitNoDocsOpt, formatOpt
};
commitCmd.SetHandler(async (string? message, bool all, bool amend, string? docsMessage, bool noDocs, string format) =>
{
    var result = await CommitCurrentWork(message, all, amend, docsMessage, noDocs, createPointerCommitIfNeeded: false);
    Render(format, result);
}, commitMessageOpt, commitAllOpt, commitAmendOpt, commitDocsMessageOpt, commitNoDocsOpt, formatOpt);

var pushForceOpt = new Option<bool>("--force", "Use --force-with-lease");
var pushNoDocsOpt = new Option<bool>("--no-docs", "Skip docs submodule handling");
var pushCmd = new Command("push", "Push the current branch, including docs handling")
{
    pushForceOpt, pushNoDocsOpt, formatOpt
};
pushCmd.SetHandler(async (bool force, bool noDocs, string format) =>
{
    var result = await PushCurrentWork(force, noDocs);
    Render(format, result);
}, pushForceOpt, pushNoDocsOpt, formatOpt);

var syncMessageOpt = new Option<string?>("--message", () => null, "Commit message");
var syncDocsMessageOpt = new Option<string?>("--docs-message", () => null, "Docs submodule commit message");
var syncCmd = new Command("sync", "Commit and push in one step")
{
    syncMessageOpt, syncDocsMessageOpt, formatOpt
};
syncCmd.SetHandler(async (string? message, string? docsMessage, string format) =>
{
    var commitResult = await CommitCurrentWork(message, all: true, amend: false, docsMessage, noDocs: false, createPointerCommitIfNeeded: false);
    var pushResult = await PushCurrentWork(force: false, noDocs: false);
    Render(format, new SyncResult(commitResult, pushResult));
}, syncMessageOpt, syncDocsMessageOpt, formatOpt);

var sonarProjectOpt = new Option<string?>("--project", () => null, "SonarQube project key");
var sonarSeverityOpt = new Option<string?>("--severity", () => null, "Comma-separated severities");
var sonarMaxOpt = new Option<int>("--max", () => 20, "Maximum issues");
var sonarCmd = new Command("sonar", "Show SonarQube issues")
{
    sonarProjectOpt, sonarSeverityOpt, sonarMaxOpt, formatOpt
};
sonarCmd.SetHandler(async (string? project, string? severity, int max, string format) =>
{
    var actualProject = project ?? throw new InvalidOperationException("--project is required for now.");
    var output = await RunScriptRaw(sonarScript, allowFailure: false, "issues", "--project", actualProject, "--max-results", max.ToString(), "--format", format,
        severity is null ? null : "--severity", severity);
    Console.WriteLine(output.StdOut);
}, sonarProjectOpt, sonarSeverityOpt, sonarMaxOpt, formatOpt);

var rootCmd = new RootCommand("Flow Manager porcelain commands")
{
    workCmd,
    taskCmd,
    prCmd,
    pipelineCmd,
    todoCmd,
    contextCmd,
    commitCmd,
    pushCmd,
    syncCmd,
    sonarCmd
};
try
{
    return await rootCmd.InvokeAsync(args);
}
catch (Exception ex)
{
    Console.Error.WriteLine(ex.Message);
    return 1;
}

// ── helpers ────────────────────────────────────────────────────────────────

async Task<CommitFlowResult> CommitCurrentWork(string? message, bool all, bool amend, string? docsMessage, bool noDocs, bool createPointerCommitIfNeeded)
{
    var ctx = ParseBranchContext();
    string? resolvedMessage = message;
    if (string.IsNullOrWhiteSpace(resolvedMessage))
    {
        if (!ctx.IsActivity)
            throw new InvalidOperationException("Commit message is required on a baseline branch.");
        var wi = await RunAdoJson("wi", "get", "--id", ctx.WorkItemId!.Value.ToString(), "--format", "json");
        resolvedMessage = $"[#{ctx.WorkItemId}] {ExtractField(wi.GetProperty("fields"), "System.Title")}: update";
    }

    DocsOperationResult? docsResult = null;
    if (!noDocs)
        docsResult = await EnsureDocsReadyForParentCommit(resolvedMessage, docsMessage, createPointerCommitIfNeeded: false);

    var status = await RunGitJson("status", "--porcelain", "--format", "json");
    if (status.GetProperty("clean").GetBoolean())
        return new CommitFlowResult(docsResult, null, "Nothing to commit. Working tree clean.");

    var commit = await RunGitJson("commit", "--message", resolvedMessage!, "--format", "json",
        all ? "--all" : null,
        amend ? "--amend" : null);

    return new CommitFlowResult(docsResult, ToCommitSummary(commit), null);
}

async Task<PushFlowResult> PushCurrentWork(bool force, bool noDocs)
{
    DocsOperationResult? docsResult = null;
    if (!noDocs)
        docsResult = await EnsureDocsReadyForParentCommit("chore: update _docs submodule pointer", "docs: update", createPointerCommitIfNeeded: true);

    var status = await RunGitJson("status", "--porcelain", "--format", "json");
    CommitSummary? pointerCommit = null;
    if (!status.GetProperty("clean").GetBoolean())
    {
        var commit = await RunGitJson("commit", "--message", "chore: update _docs submodule pointer", "--format", "json");
        pointerCommit = ToCommitSummary(commit);
    }

    var push = await RunGitJson("push", "--format", "json", force ? "--force" : null);
    return new PushFlowResult(docsResult, pointerCommit, push.GetProperty("pushed").GetBoolean(), force);
}

async Task<DocsOperationResult?> EnsureDocsReadyForParentCommit(string mainMessage, string? docsMessage, bool createPointerCommitIfNeeded)
{
    var inspection = await TryRunGitJson("submodule-inspect", "--path", docsSubmodule, "--format", "json");
    if (inspection is null || !inspection.Value.GetProperty("exists").GetBoolean())
        return null;

    var hasChanges = !inspection.Value.GetProperty("clean").GetBoolean();
    var ahead = inspection.Value.GetProperty("ahead").GetInt32();
    CommitSummary? docsCommit = null;

    if (hasChanges)
    {
        await RunGitJson("stage-all-in", "--path", docsSubmodule, "--format", "json");
        var committed = await RunGitJson("commit-in", "--path", docsSubmodule, "--message", docsMessage ?? $"docs: {mainMessage}", "--format", "json");
        docsCommit = ToCommitSummary(committed);
        ahead = Math.Max(1, ahead);
    }

    if (ahead > 0)
        await RunGitJson("push-in", "--path", docsSubmodule, "--format", "json");

    if (hasChanges || ahead > 0)
        await RunGitJson("stage", "--path", docsSubmodule, "--format", "json");

    if (createPointerCommitIfNeeded)
    {
        var status = await RunGitJson("status", "--porcelain", "--format", "json");
        if (!status.GetProperty("clean").GetBoolean())
        {
            var pointer = await RunGitJson("commit", "--message", "chore: update _docs submodule pointer", "--format", "json");
            return new DocsOperationResult(docsCommit, ToCommitSummary(pointer), ahead > 0 || hasChanges);
        }
    }

    return new DocsOperationResult(docsCommit, null, ahead > 0 || hasChanges);
}

async Task EnsureActivityInvariants(string repo, BranchContext ctx)
{
    var wi = await RunAdoJson("wi", "get", "--id", ctx.WorkItemId!.Value.ToString(), "--format", "json");
    var title = ExtractField(wi.GetProperty("fields"), "System.Title");
    var state = ExtractField(wi.GetProperty("fields"), "System.State");
    if (!string.Equals(state, "Active", StringComparison.OrdinalIgnoreCase) &&
        !string.Equals(state, "Closed", StringComparison.OrdinalIgnoreCase))
    {
        await EnsureWiState(ctx.WorkItemId.Value, "Active");
    }

    if (!await RemoteBranchExists(repo, ctx.BranchName))
        throw new InvalidOperationException($"Remote branch '{ctx.BranchName}' is missing and cannot be safely recreated from the current context.");

    var pr = await EnsureSingleActivePr(repo, ctx.BranchName, title, defaultTarget, ctx.WorkItemId.Value, createIfMissing: true, draft: true);
    await RunAdoJson("wi", "link", "--id", ctx.WorkItemId.Value.ToString(), "--repo", repo, "--branch", ctx.BranchName, "--pr", pr.GetProperty("pullRequestId").GetInt32().ToString(), "--format", "json");
}

async Task<JsonElement> EnsureSingleActivePr(string repo, string branch, string? title, string target, int wiId, bool createIfMissing, bool draft)
{
    var list = await RunAdoJson("pr", "list", "--repo", repo, "--source", branch, "--status", "active", "--format", "json");
    var prs = list.GetProperty("prs").EnumerateArray().Select(x => x.Clone()).ToList();
    if (prs.Count > 1)
        throw new InvalidOperationException($"Multiple active PRs exist for branch '{branch}'.");
    if (prs.Count == 1)
        return prs[0];
    if (!createIfMissing)
        throw new InvalidOperationException($"No active PR exists for branch '{branch}'.");

    var created = await RunAdoJson("pr", "create", "--repo", repo, "--source", branch, "--target", target, "--title", title ?? branch, "--draft", "--work-item-id", wiId.ToString(), "--format", "json");
    return created.GetProperty("pr");
}

async Task<bool> RemoteBranchExists(string repo, string branch)
{
    var exists = await RunScriptRaw(adoScript, allowFailure: true, "refs", "exists", "--repo", repo, "--name", branch);
    return exists.ExitCode == 0;
}

async Task EnsureWiState(int wiId, string state)
{
    var current = await RunAdoJson("wi", "get", "--id", wiId.ToString(), "--format", "json");
    var currentState = ExtractField(current.GetProperty("fields"), "System.State");
    if (!string.Equals(currentState, state, StringComparison.OrdinalIgnoreCase))
        await RunAdoJson("wi", "update", "--id", wiId.ToString(), "--state", state, "--format", "json");
}

async Task<ResolvedId> ResolveId(string repo, string raw)
{
    raw = raw.Trim();

    var branchMatch = Regex.Match(raw, @"^(feature|fix)/(\d+)-");
    if (branchMatch.Success)
        return new ResolvedId(int.Parse(branchMatch.Groups[2].Value), null, "branch");

    if (Regex.IsMatch(raw, @"^(w-|wi-?|w)\d+$", RegexOptions.IgnoreCase))
        return new ResolvedId(ParseTrailingInt(raw), null, "wi");
    if (Regex.IsMatch(raw, @"^(pr-|p-)\d+$", RegexOptions.IgnoreCase))
        return new ResolvedId(null, ParseTrailingInt(raw), "pr");

    if (int.TryParse(raw, out var n))
    {
        var wiTask = TryRunAdoJson("wi", "get", "--id", n.ToString(), "--format", "json");
        var prTask = TryRunAdoJson("pr", "get", "--repo", repo, "--id", n.ToString(), "--format", "json");
        await Task.WhenAll(wiTask, prTask);

        if (wiTask.Result.HasValue && prTask.Result.HasValue)
            throw new InvalidOperationException($"Identifier '{raw}' matches both a WI and a PR.");
        if (wiTask.Result.HasValue)
            return new ResolvedId(n, null, "wi");
        if (prTask.Result.HasValue)
            return new ResolvedId(null, n, "pr");

        throw new InvalidOperationException($"Identifier '{raw}' is neither a known WI nor a PR.");
    }

    throw new InvalidOperationException($"Cannot parse identifier '{raw}'.");
}

async Task<int?> ResolvePrIdFromWorkItem(string repo, int wiId)
{
    var wi = await RunAdoJson("wi", "get", "--id", wiId.ToString(), "--format", "json");
    var prId = TryExtractPrIdFromWi(wi);
    if (prId.HasValue)
        return prId;

    var fields = wi.GetProperty("fields");
    var title = ExtractField(fields, "System.Title");
    var wiType = ExtractField(fields, "System.WorkItemType");
    var branch = BranchNameForWorkItem(wiId, title, wiType);
    var list = await RunAdoJson("pr", "list", "--repo", repo, "--source", branch, "--status", "all", "--format", "json");
    var prs = list.GetProperty("prs").EnumerateArray().Select(x => x.Clone()).ToList();
    return prs.Count > 0 ? prs[0].GetProperty("pullRequestId").GetInt32() : null;
}

int? TryExtractPrIdFromWi(JsonElement wi)
{
    if (!wi.TryGetProperty("relations", out var relations))
        return null;

    foreach (var relation in relations.EnumerateArray())
    {
        var url = relation.TryGetProperty("url", out var urlEl) ? urlEl.GetString() : null;
        if (string.IsNullOrWhiteSpace(url))
            continue;

        var match = Regex.Match(url, @"PullRequestId/.+%2F(\d+)$");
        if (match.Success && int.TryParse(match.Groups[1].Value, out var prId))
            return prId;
    }

    return null;
}

async Task<string?> RestoreNamedStash(string filter)
{
    var list = await RunGitJson("stash-list", "--filter", filter, "--format", "json");
    if (list.GetProperty("count").GetInt32() == 0)
        return null;

    await RunGitJson("stash-pop", "--name", filter, "--format", "json");
    return "restored";
}

async Task<JsonElement?> FindExistingWorkItem(string title, string wiType)
{
    var project = Environment.GetEnvironmentVariable("ADO_PROJECT")
        ?? throw new InvalidOperationException("ADO_PROJECT environment variable is not set.");
    var escapedTitle = title.Replace("'", "''");
    var query = $"SELECT [System.Id] FROM WorkItems WHERE [System.TeamProject] = '{project}' AND [System.Title] = '{escapedTitle}' AND [System.WorkItemType] = '{wiType.Replace("'", "''")}'";
    var results = await RunAdoJson("wi", "search", "--query", query, "--max-results", "5", "--format", "json");
    if (results.GetProperty("count").GetInt32() == 0)
        return null;

    var item = results.GetProperty("items")[0];
    return await RunAdoJson("wi", "get", "--id", item.GetProperty("id").GetInt32().ToString(), "--format", "json");
}

async Task<string> AppendSonarIssues(string? description, string project)
{
    var sonar = await RunScriptJson(sonarScript, "issues", "--project", project, "--max-results", "20", "--format", "json");
    var issues = sonar.GetProperty("issues").EnumerateArray().ToList();
    var sb = new StringBuilder(description ?? string.Empty);
    sb.Append($"<h2>SonarQube Issues — {project}</h2>");
    if (issues.Count == 0)
    {
        sb.Append("<p><em>No open issues found.</em></p>");
    }
    else
    {
        sb.Append("<ul>");
        foreach (var issue in issues)
        {
            sb.Append($"<li><strong>{ExtractString(issue, "severity")}</strong>: {ExtractString(issue, "message")} <em>({ExtractString(issue, "component")})</em></li>");
        }
        sb.Append("</ul>");
    }
    return sb.ToString();
}

Command BuildTodoShowCommand()
{
    var allOpt = new Option<bool>("--all", "Include closed items");
    var detailOpt = new Option<bool>("--detail", "Include descriptions");
    var cmd = new Command("show", "Show todos")
    {
        allOpt, detailOpt, formatOpt
    };
    cmd.SetHandler(async (bool all, bool detail, string format) =>
    {
        var ctx = EnsureActivity(ParseBranchContext());
        await EnsureActivityInvariants(ResolveRepo(), ctx);
        var output = await RunScriptRaw(todoScript, allowFailure: false, "show", "--wi-id", ctx.WorkItemId!.Value.ToString(), "--format", format,
            all ? "--all" : null,
            detail ? "--detail" : null);
        Console.WriteLine(output.StdOut);
    }, allOpt, detailOpt, formatOpt);
    return cmd;
}

Command BuildTodoNewCommand()
{
    var titleOpt = new Option<string>("--title", "Todo title") { IsRequired = true };
    var descriptionOpt = new Option<string?>("--description", () => null, "Description");
    var assignedOpt = new Option<string?>("--assigned-to", () => null, "Assigned-to");
    var pickOpt = new Option<bool>("--pick", "Set Active immediately");
    var cmd = new Command("new", "Create a todo")
    {
        titleOpt, descriptionOpt, assignedOpt, pickOpt, formatOpt
    };
    cmd.SetHandler(async (string title, string? description, string? assignedTo, bool pick, string format) =>
    {
        var ctx = EnsureActivity(ParseBranchContext());
        await EnsureActivityInvariants(ResolveRepo(), ctx);
        var output = await RunScriptRaw(todoScript, allowFailure: false, "new", "--wi-id", ctx.WorkItemId!.Value.ToString(), "--title", title, "--format", format,
            description is null ? null : "--description", description,
            assignedTo is null ? null : "--assigned-to", assignedTo,
            pick ? "--pick" : null);
        Console.WriteLine(output.StdOut);
    }, titleOpt, descriptionOpt, assignedOpt, pickOpt, formatOpt);
    return cmd;
}

Command BuildTodoRefCommand(string name, string description)
{
    var refArg = new Argument<string>("ref", "Task id or title fragment");
    var cmd = new Command(name, description) { refArg, formatOpt };
    cmd.SetHandler(async (string reference, string format) =>
    {
        var ctx = EnsureActivity(ParseBranchContext());
        await EnsureActivityInvariants(ResolveRepo(), ctx);
        var output = await RunScriptRaw(todoScript, allowFailure: false, name, "--wi-id", ctx.WorkItemId!.Value.ToString(), reference, "--format", format);
        Console.WriteLine(output.StdOut);
    }, refArg, formatOpt);
    return cmd;
}

Command BuildTodoUpdateCommand()
{
    var refArg = new Argument<string>("ref", "Task id or title fragment");
    var titleOpt = new Option<string?>("--title", () => null, "New title");
    var descriptionOpt = new Option<string?>("--description", () => null, "New description");
    var assignedOpt = new Option<string?>("--assigned-to", () => null, "New assigned-to");
    var stateOpt = new Option<string?>("--state", () => null, "New state");
    var cmd = new Command("update", "Update a todo")
    {
        refArg, titleOpt, descriptionOpt, assignedOpt, stateOpt, formatOpt
    };
    cmd.SetHandler(async (string reference, string? title, string? description, string? assignedTo, string? state, string format) =>
    {
        var ctx = EnsureActivity(ParseBranchContext());
        await EnsureActivityInvariants(ResolveRepo(), ctx);
        var output = await RunScriptRaw(todoScript, allowFailure: false, "update", "--wi-id", ctx.WorkItemId!.Value.ToString(), reference, "--format", format,
            title is null ? null : "--title", title,
            description is null ? null : "--description", description,
            assignedTo is null ? null : "--assigned-to", assignedTo,
            state is null ? null : "--state", state);
        Console.WriteLine(output.StdOut);
    }, refArg, titleOpt, descriptionOpt, assignedOpt, stateOpt, formatOpt);
    return cmd;
}

Command BuildTodoNextCommand()
{
    var pickOpt = new Option<bool>("--pick", "Set Active immediately");
    var cmd = new Command("next", "Show the next New todo")
    {
        pickOpt, formatOpt
    };
    cmd.SetHandler(async (bool pick, string format) =>
    {
        var ctx = EnsureActivity(ParseBranchContext());
        await EnsureActivityInvariants(ResolveRepo(), ctx);
        var output = await RunScriptRaw(todoScript, allowFailure: false, "next", "--wi-id", ctx.WorkItemId!.Value.ToString(), "--format", format,
            pick ? "--pick" : null);
        Console.WriteLine(output.StdOut);
    }, pickOpt, formatOpt);
    return cmd;
}

string ResolveRepo()
{
    var envRepo = Environment.GetEnvironmentVariable("FM_REPO");
    if (!string.IsNullOrWhiteSpace(envRepo))
        return envRepo;

    var remote = RunScriptRaw(gitScript, allowFailure: false, "remote-get-url", "--name", "origin", "--format", "json").GetAwaiter().GetResult();
    using var doc = JsonDocument.Parse(remote.StdOut);
    var url = doc.RootElement.GetProperty("url").GetString()
        ?? throw new InvalidOperationException("Remote origin URL not found.");

    var https = Regex.Match(url, @"dev\.azure\.com/[^/]+/[^/]+/_git/([^/?#]+)");
    if (https.Success)
        return Uri.UnescapeDataString(https.Groups[1].Value);

    var ssh = Regex.Match(url, @"ssh\.dev\.azure\.com[:/]v3/[^/]+/[^/]+/([^/?#]+)");
    if (ssh.Success)
        return Uri.UnescapeDataString(ssh.Groups[1].Value);

    throw new InvalidOperationException($"Cannot parse ADO repository from origin URL '{url}'.");
}

BranchContext ParseBranchContext()
{
    var current = RunGitJson("branch-current", "--format", "json").GetAwaiter().GetResult();
    var branch = current.GetProperty("name").GetString()
        ?? throw new InvalidOperationException("Current branch not found.");

    var match = Regex.Match(branch, @"^(feature|fix)/(\d+)-(.+)$");
    if (!match.Success)
        return new BranchContext(false, branch, null, null, null);

    return new BranchContext(true, branch, match.Groups[1].Value, int.Parse(match.Groups[2].Value), match.Groups[3].Value);
}

BranchContext EnsureActivity(BranchContext ctx)
{
    if (!ctx.IsActivity)
        throw new InvalidOperationException($"Current branch '{ctx.BranchName}' is not an activity branch.");
    return ctx;
}

string BranchNameForWorkItem(int wiId, string title, string wiType)
{
    var prefix = wiType.Equals("Bug", StringComparison.OrdinalIgnoreCase) ? "fix" : "feature";
    return $"{prefix}/{wiId}-{Slugify(title)}";
}

string Slugify(string value)
{
    var slug = Regex.Replace(value.ToLowerInvariant(), @"[^a-z0-9]+", "-").Trim('-');
    if (slug.Length > 40)
        slug = slug[..40].TrimEnd('-');
    return slug;
}

static int ParseTrailingInt(string value)
{
    var digits = new string(value.Where(char.IsDigit).ToArray());
    return int.Parse(digits);
}

static string StripRefsHeads(string value)
    => value.StartsWith("refs/heads/", StringComparison.OrdinalIgnoreCase) ? value["refs/heads/".Length..] : value;

static string ExtractField(JsonElement fields, string name)
{
    if (!fields.TryGetProperty(name, out var value))
        return "—";

    return value.ValueKind switch
    {
        JsonValueKind.Null => "—",
        JsonValueKind.Object => value.TryGetProperty("displayName", out var displayName) ? displayName.GetString() ?? "—" : "—",
        _ => value.GetString() ?? "—"
    };
}

static string ExtractString(JsonElement node, string name)
{
    if (!node.TryGetProperty(name, out var value))
        return "—";
    return value.ValueKind switch
    {
        JsonValueKind.Null => "—",
        JsonValueKind.Object => value.TryGetProperty("displayName", out var displayName) ? displayName.GetString() ?? "—" : "—",
        _ => value.GetString() ?? "—"
    };
}

CommitSummary ToCommitSummary(JsonElement json)
    => new(
        json.GetProperty("shortCommit").GetString() ?? "—",
        json.GetProperty("subject").GetString() ?? "—",
        json.GetProperty("stats").EnumerateArray().Select(x => x.GetString() ?? string.Empty).ToList(),
        json.TryGetProperty("amended", out var amendedEl) && amendedEl.GetBoolean());

async Task<JsonElement> RunAdoJson(params string?[] args) => await RunScriptJson(adoScript, args);
async Task<JsonElement> RunGitJson(params string?[] args) => await RunScriptJson(gitScript, args);
async Task<JsonElement?> TryRunAdoJson(params string?[] args) => await TryRunScriptJson(adoScript, args);
async Task<JsonElement?> TryRunGitJson(params string?[] args) => await TryRunScriptJson(gitScript, args);
async Task<JsonElement> RunTodoJson(params string?[] args) => await RunScriptJson(todoScript, args);

async Task<JsonElement> RunScriptJson(string script, params string?[] args)
{
    var result = await RunScriptRaw(script, allowFailure: false, args);
    using var doc = JsonDocument.Parse(result.StdOut);
    return doc.RootElement.Clone();
}

async Task<JsonElement> RunJsonAllowFailure(string script, params string?[] args)
{
    var result = await RunScriptRaw(script, allowFailure: true, args);
    if (string.IsNullOrWhiteSpace(result.StdOut))
        throw new InvalidOperationException(string.IsNullOrWhiteSpace(result.StdErr) ? $"{Path.GetFileName(script)} failed." : result.StdErr);

    using var doc = JsonDocument.Parse(result.StdOut);
    return doc.RootElement.Clone();
}

async Task<JsonElement?> TryRunScriptJson(string script, params string?[] args)
{
    var result = await RunScriptRaw(script, allowFailure: true, args);
    if (result.ExitCode != 0)
        return null;

    using var doc = JsonDocument.Parse(result.StdOut);
    return doc.RootElement.Clone();
}

async Task<ScriptResult> RunScriptRaw(string script, bool allowFailure, params string?[] rawArgs)
{
    var args = rawArgs.Where(x => !string.IsNullOrWhiteSpace(x)).Select(x => x!).ToList();
    var psi = new ProcessStartInfo("dotnet")
    {
        RedirectStandardOutput = true,
        RedirectStandardError = true,
        UseShellExecute = false
    };

    psi.ArgumentList.Add("run");
    psi.ArgumentList.Add(script);
    psi.ArgumentList.Add("--");
    foreach (var arg in args)
        psi.ArgumentList.Add(arg);

    using var proc = Process.Start(psi) ?? throw new InvalidOperationException($"Failed to start {Path.GetFileName(script)}.");
    var stdout = await proc.StandardOutput.ReadToEndAsync();
    var stderr = await proc.StandardError.ReadToEndAsync();
    await proc.WaitForExitAsync();

    if (!allowFailure && proc.ExitCode != 0)
    {
        if (!string.IsNullOrWhiteSpace(stderr))
            throw new InvalidOperationException(stderr.Trim());
        throw new InvalidOperationException($"{Path.GetFileName(script)} failed.");
    }

    return new ScriptResult(proc.ExitCode, stdout.Trim(), stderr.Trim());
}

void Render(string format, object data)
{
    var output = format.ToLowerInvariant() switch
    {
        "yaml" => yaml.Serialize(ToYamlObject(data)),
        "markdown" => RenderMarkdown(data),
        _ => JsonSerializer.Serialize(data, jsonOpts)
    };
    Console.WriteLine(output);
}

string RenderMarkdown(object data) => data switch
{
    WorkActivityResult x => RenderWorkActivity(x),
    WorkClosedResult x => RenderWorkClosed(x),
    WorkListResult x => RenderWorkList(x),
    ContextBaselineResult x => RenderContextBaseline(x),
    ContextActivityResult x => RenderContextActivity(x),
    TaskHoldResult x => RenderTaskHold(x),
    TaskUpdateResult x => RenderTaskUpdate(x),
    TaskCompleteResult x => RenderTaskComplete(x),
    TaskSyncResult x => RenderTaskSync(x),
    TaskSyncCheckResult x => RenderTaskSyncCheck(x),
    TaskSyncConflictResult x => RenderTaskSyncConflict(x),
    PrShowResult x => RenderPrShow(x),
    PrMergeResult x => RenderPrMerge(x),
    PrMergeErrorResult x => RenderPrMergeError(x),
    PrReviewResult x => RenderPrReview(x),
    PipelineListResult x => RenderPipelineList(x),
    PipelineRunSummaryResult x => RenderPipelineRunSummary(x),
    PipelineStatusResult x => RenderPipelineStatus(x),
    CommitFlowResult x => RenderCommitFlow(x),
    PushFlowResult x => RenderPushFlow(x),
    SyncResult x => RenderSync(x),
    ErrorResult x => $"## {x.Title}\n\n{x.Message}\n",
    _ => throw new NotSupportedException($"No markdown renderer for {data.GetType().Name}")
};

object? ToYamlObject(object? value) => value switch
{
    JsonElement el => el.ValueKind switch
    {
        JsonValueKind.Object => el.EnumerateObject().ToDictionary(p => p.Name, p => ToYamlObject(p.Value)),
        JsonValueKind.Array => el.EnumerateArray().Select(ToYamlObject).ToList(),
        JsonValueKind.String => el.GetString(),
        JsonValueKind.Number => el.TryGetInt64(out var l) ? l : el.GetDouble(),
        JsonValueKind.True => true,
        JsonValueKind.False => false,
        _ => null
    },
    _ => value
};

string RenderWorkActivity(WorkActivityResult x)
{
    var heading = x.Action == "loaded" ? "Activity Loaded" : "New Activity Started";
    var sb = new StringBuilder();
    sb.AppendLine($"## {heading}");
    sb.AppendLine();
    sb.AppendLine("| | |");
    sb.AppendLine("|-|---|");
    sb.AppendLine($"| Work Item | #{x.Id} — {x.Title} |");
    sb.AppendLine($"| Type      | {x.Type} |");
    sb.AppendLine($"| State     | {x.State} |");
    sb.AppendLine($"| Branch    | `{x.Branch}` |");
    sb.AppendLine($"| PR        | #{x.PrId} ({x.PrState}) |");
    sb.AppendLine($"| Mergeable | {x.Mergeable} |");
    sb.AppendLine($"| Target    | `{x.Target}` |");
    if (!string.IsNullOrWhiteSpace(x.StashRestored))
        sb.AppendLine($"| Stash restored | {x.StashRestored} |");
    return sb.ToString();
}

string RenderWorkClosed(WorkClosedResult x)
{
    var sb = new StringBuilder();
    sb.AppendLine($"## Work Item #{x.Id} — {x.State}");
    sb.AppendLine();
    sb.AppendLine($"  Title  {x.Title}");
    sb.AppendLine($"  State  {x.State}");
    if (x.MergedPrId.HasValue)
        sb.AppendLine($"  PR     #{x.MergedPrId.Value}");
    sb.AppendLine();
    sb.AppendLine("  No branch switch — WI is closed.");
    return sb.ToString();
}

string RenderWorkList(WorkListResult x)
{
    var sb = new StringBuilder();
    sb.AppendLine($"# Work Items ({x.Count})");
    sb.AppendLine();
    if (x.Count == 0)
    {
        sb.AppendLine("_No matching work items._");
        return sb.ToString();
    }
    sb.AppendLine("| ID | Type | State | Title | Assigned To |");
    sb.AppendLine("|----|------|-------|-------|-------------|");
    foreach (var item in x.Items)
    {
        var id = item.GetProperty("id").GetInt32();
        var fields = item.GetProperty("fields");
        sb.AppendLine($"| {id} | {ExtractField(fields, "System.WorkItemType")} | {ExtractField(fields, "System.State")} | {ExtractField(fields, "System.Title")} | {ExtractField(fields, "System.AssignedTo")} |");
    }
    return sb.ToString();
}

string RenderContextBaseline(ContextBaselineResult x)
{
    var sb = new StringBuilder();
    sb.AppendLine($"## Context — `{x.Branch}` (baseline)");
    sb.AppendLine();
    sb.AppendLine("Last commits:");
    foreach (var commit in x.RecentCommits)
        sb.AppendLine($"- {commit}");
    return sb.ToString();
}

string RenderContextActivity(ContextActivityResult x)
{
    var sb = new StringBuilder();
    sb.AppendLine($"## Context — `{x.Branch}`");
    sb.AppendLine();

    if (!x.OnlyPr && !x.OnlyGit && !x.OnlyPipeline && x.Wi.HasValue)
    {
        var fields = x.Wi.Value.GetProperty("fields");
        sb.AppendLine("### Work Item");
        sb.AppendLine("| | |");
        sb.AppendLine("|-|---|");
        sb.AppendLine($"| ID | #{x.Wi.Value.GetProperty("id").GetInt32()} |");
        sb.AppendLine($"| Title | {ExtractField(fields, "System.Title")} |");
        sb.AppendLine($"| State | {ExtractField(fields, "System.State")} |");
        sb.AppendLine($"| Assigned | {ExtractField(fields, "System.AssignedTo")} |");
        sb.AppendLine();
    }

    if (!x.OnlyWi && !x.OnlyGit && !x.OnlyPipeline)
    {
        sb.AppendLine("### Pull Request");
        if (x.Pr.HasValue)
        {
            var isDraft = x.Pr.Value.TryGetProperty("isDraft", out var draftEl) && draftEl.GetBoolean();
            sb.AppendLine("| | |");
            sb.AppendLine("|-|---|");
            sb.AppendLine($"| PR | #{x.PrId} |");
            sb.AppendLine($"| State | {(isDraft ? "draft" : ExtractString(x.Pr.Value, "status"))} |");
            sb.AppendLine($"| Mergeable | {ExtractString(x.Pr.Value, "mergeStatus")} |");
            sb.AppendLine($"| Target | `{StripRefsHeads(ExtractString(x.Pr.Value, "targetRefName"))}` |");
        }
        else
        {
            sb.AppendLine("_No active PR for this branch._");
        }
        sb.AppendLine();
    }

    if (!x.OnlyWi && !x.OnlyPr && !x.OnlyPipeline)
    {
        sb.AppendLine("### Git");
        sb.AppendLine("| | |");
        sb.AppendLine("|-|---|");
        sb.AppendLine($"| Ahead | {x.Ahead} commits |");
        sb.AppendLine($"| Behind | {x.Behind} commits |");
        sb.AppendLine($"| Local | {(x.Clean ? "clean" : "dirty")} |");
        sb.AppendLine();
    }

    if (!x.OnlyWi && !x.OnlyPr && !x.OnlyGit)
    {
        sb.AppendLine("### CI");
        if (x.LatestRun.HasValue)
        {
            sb.AppendLine("| | |");
            sb.AppendLine("|-|---|");
            sb.AppendLine($"| Pipeline | {x.PipelineName ?? "—"} (#{x.PipelineId?.ToString() ?? "—"}) |");
            var status = ExtractString(x.LatestRun.Value, "status");
            var result = x.LatestRun.Value.TryGetProperty("result", out var resultEl) ? resultEl.GetString() : null;
            sb.AppendLine($"| Last run | #{ExtractString(x.LatestRun.Value, "id")} — {(status == "completed" ? result ?? status : status)} |");
        }
        else
        {
            sb.AppendLine("_No CI runs found for this branch._");
        }
        sb.AppendLine();
    }

    if (!x.OnlyWi && !x.OnlyPr && !x.OnlyGit && !x.OnlyPipeline && x.TodoSnapshot.HasValue)
    {
        var snapshot = x.TodoSnapshot.Value;
        var active = snapshot.GetProperty("active").EnumerateArray().ToList();
        var @new = snapshot.GetProperty("new").EnumerateArray().ToList();
        var closed = snapshot.GetProperty("closed").EnumerateArray().ToList();
        if (active.Count + @new.Count + closed.Count > 0)
        {
            sb.AppendLine("### Todos");
            foreach (var todo in active)
                sb.AppendLine($"  ●  #{todo.GetProperty("id").GetInt32()}  {todo.GetProperty("title").GetString()}".PadRight(58) + "Active");
            foreach (var todo in @new)
                sb.AppendLine($"  ○  #{todo.GetProperty("id").GetInt32()}  {todo.GetProperty("title").GetString()}");
            sb.AppendLine("  -----------------------------------------");
            sb.AppendLine($"  {closed.Count} done · {active.Count} active · {@new.Count} open · {active.Count + @new.Count + closed.Count} total  (run `fm todo show` for detail)");
        }
    }

    return sb.ToString();
}

string RenderTaskHold(TaskHoldResult x)
{
    if (!string.IsNullOrWhiteSpace(x.Note))
        return $"## Task Hold\n\n  {x.Note}\n";
    return $"## Task Hold\n\n| | |\n|-|---|\n| Branch pushed | `{x.Branch}` |\n| Stash | {(x.Stash is null ? "—" : $"`{x.Stash}` saved")} |\n| Now on | `{x.NowOn}` |\n";
}

string RenderTaskUpdate(TaskUpdateResult x)
{
    var fields = x.Wi.GetProperty("fields");
    return $"## Task Updated — #{x.WiId}\n\n| Field | Value |\n|-------|-------|\n| Title | {ExtractField(fields, "System.Title")} |\n| State | {ExtractField(fields, "System.State")} |\n| Assigned | {ExtractField(fields, "System.AssignedTo")} |\n| Tags | {ExtractField(fields, "System.Tags")} |\n";
}

string RenderTaskComplete(TaskCompleteResult x)
    => $"## Activity Complete\n\n| | |\n|-|---|\n| WI | #{x.WiId} — {x.WiState} |\n| PR | #{x.PrId} — {x.PrState} |\n| Now on | `{x.NowOn}` (up to date) |\n";

string RenderTaskSync(TaskSyncResult x)
{
    var sb = new StringBuilder();
    sb.AppendLine($"## Task Sync — `{x.Branch}`");
    sb.AppendLine();
    sb.AppendLine($"  Strategy   {x.Strategy}");
    sb.AppendLine($"  From       origin/{x.Target}  ({x.Behind} commits behind)");
    sb.AppendLine($"  Result     {(x.Behind == 0 ? "already up to date" : "clean merge")}  ->  {(x.Pushed ? "pushed" : "not pushed")}");
    if (x.CommitsMerged.Count > 0)
    {
        sb.AppendLine();
        sb.AppendLine("  Commits merged:");
        foreach (var commit in x.CommitsMerged)
            sb.AppendLine($"  - {commit}");
    }
    return sb.ToString();
}

string RenderTaskSyncCheck(TaskSyncCheckResult x)
{
    var sb = new StringBuilder();
    sb.AppendLine($"## Task Sync Check — `{x.Branch}`");
    sb.AppendLine();
    sb.AppendLine($"  Branch is {x.Behind} commits behind origin/{x.Target}, {x.Ahead} commits ahead.");
    if (x.BehindCommits.Count > 0)
    {
        sb.AppendLine();
        sb.AppendLine("  Behind:");
        foreach (var commit in x.BehindCommits)
            sb.AppendLine($"  - {commit}");
    }
    if (x.AheadCommits.Count > 0)
    {
        sb.AppendLine();
        sb.AppendLine("  Ahead:");
        foreach (var commit in x.AheadCommits)
            sb.AppendLine($"  - {commit}");
    }
    sb.AppendLine();
    sb.AppendLine("  Run `fm task sync` to merge, or `fm task sync --rebase` to rebase.");
    return sb.ToString();
}

string RenderTaskSyncConflict(TaskSyncConflictResult x)
{
    var sb = new StringBuilder();
    sb.AppendLine("## Task Sync — CONFLICT");
    sb.AppendLine();
    sb.AppendLine($"  Strategy   {x.Strategy}");
    sb.AppendLine($"  From       origin/{x.Target}");
    sb.AppendLine();
    sb.AppendLine("  Conflicting files:");
    foreach (var conflict in x.ConflictingFiles)
        sb.AppendLine($"  - {conflict}");
    sb.AppendLine();
    sb.AppendLine("  Resolve conflicts manually, then run `fm push`.");
    return sb.ToString();
}

string RenderPrShow(PrShowResult x)
{
    var pr = x.Pr;
    var isDraft = pr.TryGetProperty("isDraft", out var draftEl) && draftEl.GetBoolean();
    var createdBy = pr.TryGetProperty("createdBy", out var createdByNode) ? ExtractString(createdByNode, "displayName") : "—";
    return $"## PR #{x.PrId} — {ExtractString(pr, "title")}\n\n| Field | Value |\n|------|-------|\n| State | {(isDraft ? "draft" : ExtractString(pr, "status"))} |\n| Branches | `{StripRefsHeads(ExtractString(pr, "sourceRefName"))}` -> `{StripRefsHeads(ExtractString(pr, "targetRefName"))}` |\n| Created By | {createdBy} |\n| Mergeable | {ExtractString(pr, "mergeStatus")} |\n";
}

string RenderPrMerge(PrMergeResult x)
    => $"## PR Merged — #{x.PrId}\n\n| | |\n|-|---|\n| Strategy | {x.Strategy} |\n| PR | #{x.PrId} — completed |\n| WI | #{x.WiId} — Closed |\n| Merged to | `{x.MergedTo}` |\n| Commit | {x.Commit} |\n\n  Run `fm task complete` to switch to `{defaultTarget}` and pull.\n";

string RenderPrMergeError(PrMergeErrorResult x)
{
    var sb = new StringBuilder();
    sb.AppendLine("## Error — PR Not Mergeable");
    sb.AppendLine();
    sb.AppendLine($"  PR #{x.PrId} cannot be merged.");
    sb.AppendLine();
    foreach (var failure in x.Failures)
        sb.AppendLine($"  - {failure}");
    return sb.ToString();
}

string RenderPrReview(PrReviewResult x)
{
    var sb = new StringBuilder();
    sb.AppendLine($"## PR Review — #{x.PrId}");
    sb.AppendLine();
    sb.AppendLine($"  Now on branch  `{x.Branch}`");
    if (!string.IsNullOrWhiteSpace(x.OriginalBranch))
        sb.AppendLine($"  Held activity  `{x.OriginalBranch}`{(x.StashName is null ? string.Empty : $"  (stash: `{x.StashName}`)")}");
    return sb.ToString();
}

string RenderPipelineList(PipelineListResult x)
{
    var sb = new StringBuilder();
    sb.AppendLine("# Pipelines");
    sb.AppendLine();
    sb.AppendLine($"**Count:** {x.Count}");
    sb.AppendLine();
    foreach (var pipeline in x.Pipelines)
        sb.AppendLine($"- #{pipeline.GetProperty("id").GetInt32()}  {pipeline.GetProperty("name").GetString()}");
    if (!string.IsNullOrWhiteSpace(x.Note))
    {
        sb.AppendLine();
        sb.AppendLine(x.Note);
    }
    return sb.ToString();
}

string RenderPipelineRunSummary(PipelineRunSummaryResult x)
    => $"## Pipeline Run\n\n  Pipeline  #{x.PipelineId}\n  Run       #{x.RunId}\n  Branch    `{x.Branch}`\n";

string RenderPipelineStatus(PipelineStatusResult x)
    => $"## Pipeline Status\n\n  Run       #{x.RunId}\n  Pipeline  {x.PipelineName ?? "—"}\n  Status    {x.Status}\n  Result    {x.Result ?? "—"}\n  Branch    `{x.Branch}`\n";

string RenderCommitFlow(CommitFlowResult x)
{
    if (!string.IsNullOrWhiteSpace(x.Note))
        return $"## Commit\n\n{x.Note}\n";

    var sb = new StringBuilder();
    sb.AppendLine("## Commit");
    sb.AppendLine();
    if (x.Docs is not null && x.Docs.Changed)
    {
        sb.AppendLine($"  _docs  ->  {(x.Docs.Commit?.ShortCommit ?? x.Docs.PointerCommit?.ShortCommit ?? "pushed")}  {(x.Docs.Commit?.Subject ?? x.Docs.PointerCommit?.Subject ?? "updated")}");
        sb.AppendLine();
    }
    if (x.Main is not null)
    {
        sb.AppendLine($"  main   ->  {x.Main.ShortCommit}  {x.Main.Subject}");
        foreach (var stat in x.Main.Stats.TakeLast(1))
            sb.AppendLine($"           {stat}");
    }
    return sb.ToString();
}

string RenderPushFlow(PushFlowResult x)
{
    var sb = new StringBuilder();
    sb.AppendLine("## Push");
    sb.AppendLine();
    if (x.Docs is not null && x.Docs.Changed)
        sb.AppendLine("  _docs  ->  pushed");
    if (x.PointerCommit is not null)
        sb.AppendLine($"  main   ->  {x.PointerCommit.ShortCommit}  {x.PointerCommit.Subject}");
    sb.AppendLine($"  remote ->  {(x.Pushed ? "updated" : "not pushed")}{(x.Force ? " (force-with-lease)" : string.Empty)}");
    return sb.ToString();
}

string RenderSync(SyncResult x)
{
    var sb = new StringBuilder();
    sb.AppendLine("## Sync");
    sb.AppendLine();
    if (!string.IsNullOrWhiteSpace(x.Commit.Note) && x.Commit.Main is null && x.Push.PointerCommit is null && x.Push.Docs is null)
    {
        sb.AppendLine(x.Commit.Note);
        return sb.ToString();
    }
    sb.AppendLine(RenderCommitFlow(x.Commit).TrimEnd());
    sb.AppendLine();
    sb.AppendLine(RenderPushFlow(x.Push).TrimEnd());
    return sb.ToString();
}

record ScriptResult(int ExitCode, string StdOut, string StdErr);
record BranchContext(bool IsActivity, string BranchName, string? ActivityType, int? WorkItemId, string? Slug);
record ResolvedId(int? WiId, int? PrId, string Source);
record ErrorResult(string Title, string Message);
record WorkActivityResult(int Id, string Title, string Type, string State, string Branch, int PrId, string PrState, string Target, string Mergeable, string? StashRestored, string Action);
record WorkClosedResult(int Id, string Title, string State, int? MergedPrId);
record WorkListResult(int Count, List<JsonElement> Items);
record ContextBaselineResult(string Branch, List<string> RecentCommits);
record ContextActivityResult(string Branch, JsonElement? Wi, int? PrId, JsonElement? Pr, int Ahead, int Behind, bool Clean, string? PipelineName, int? PipelineId, JsonElement? LatestRun, JsonElement? TodoSnapshot, bool OnlyWi, bool OnlyPr, bool OnlyGit, bool OnlyPipeline, string Target);
record TaskHoldResult(string Branch, string? Stash, string? NowOn, string? Note);
record TaskUpdateResult(int WiId, JsonElement Wi);
record TaskCompleteResult(int WiId, string WiState, int PrId, string PrState, string NowOn);
record TaskSyncResult(string Branch, string Strategy, string Target, int Behind, bool Pushed, List<string> CommitsMerged);
record TaskSyncCheckResult(string Branch, string Target, int Ahead, int Behind, List<string> BehindCommits, List<string> AheadCommits);
record TaskSyncConflictResult(string Branch, string Strategy, string Target, List<string> ConflictingFiles);
record PrShowResult(int PrId, JsonElement Pr);
record PrMergeResult(int PrId, string Strategy, int WiId, string MergedTo, string Commit);
record PrMergeErrorResult(int PrId, List<string> Failures);
record PrReviewResult(int PrId, string Branch, string? OriginalBranch, string? StashName);
record PipelineListResult(int Count, List<JsonElement> Pipelines, string? Note);
record PipelineRunSummaryResult(int PipelineId, int RunId, string Branch, JsonElement Run);
record PipelineStatusResult(int RunId, string? PipelineName, string Status, string? Result, string Branch, JsonElement Run);
record CommitSummary(string ShortCommit, string Subject, List<string> Stats, bool Amended);
record DocsOperationResult(CommitSummary? Commit, CommitSummary? PointerCommit, bool Changed);
record CommitFlowResult(DocsOperationResult? Docs, CommitSummary? Main, string? Note);
record PushFlowResult(DocsOperationResult? Docs, CommitSummary? PointerCommit, bool Pushed, bool Force);
record SyncResult(CommitFlowResult Commit, PushFlowResult Push);
