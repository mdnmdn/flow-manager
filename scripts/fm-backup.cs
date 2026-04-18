#!/usr/bin/dotnet run
#:package DotNetEnv@3.1.1
#:package System.CommandLine@2.0.0-beta4.22272.1
#:package YamlDotNet@16.3.0
#:property PublishAot=false
#:property PublishTrimmed=false

using System.CommandLine;
using System.Diagnostics;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using DotNetEnv;
using YamlDotNet.Serialization;
using YamlDotNet.Serialization.NamingConventions;

// ── env loading ───────────────────────────────────────────────────────────────
var envPath = Path.Combine(Directory.GetCurrentDirectory(), ".env");
if (!File.Exists(envPath))
    envPath = Path.Combine(AppContext.BaseDirectory, ".env");
if (File.Exists(envPath))
    Env.Load(envPath);

var jsonOpts = new JsonSerializerOptions { WriteIndented = true };
var yaml     = new SerializerBuilder()
    .WithNamingConvention(CamelCaseNamingConvention.Instance)
    .Build();

// ── FM_* configuration ────────────────────────────────────────────────────────
var defaultFormat       = Environment.GetEnvironmentVariable("FM_FORMAT")          ?? "markdown";
var defaultMergeStrat   = Environment.GetEnvironmentVariable("FM_MERGE_STRATEGY")  ?? "squash";
var defaultTarget       = Environment.GetEnvironmentVariable("FM_DEFAULT_TARGET")  ?? "main";
var defaultWiType       = Environment.GetEnvironmentVariable("FM_DEFAULT_WI_TYPE") ?? "User Story";
var docsSubmodule       = Environment.GetEnvironmentVariable("FM_DOCS_SUBMODULE")  ?? "_docs";

// ── shared options ────────────────────────────────────────────────────────────
var formatOpt = new Option<string>("--format", () => defaultFormat, "Output format: json, yaml, markdown");

// ── command groups ────────────────────────────────────────────────────────────
var workCmd     = new Command("work",     "Manage work activities");
var taskCmd     = new Command("task",     "Manage the current task/activity");
var prCmd       = new Command("pr",       "Manage pull requests");
var pipelineCmd = new Command("pipeline", "Manage CI pipelines");
var todoCmd     = new Command("todo",     "Manage todos (child Tasks of the active WI)");

// ── PHASE_WORK_SUBCOMMANDS ────────────────────────────────────────────────────

// fm work new
var workNewTitleOpt       = new Option<string>("--title",       "Work item title")  { IsRequired = true };
var workNewDescOpt        = new Option<string?>("--description", () => null, "Work item description (HTML accepted)");
var workNewBranchOpt      = new Option<string?>("--branch",      () => null, "Branch slug suffix (defaults to slug of title)");
var workNewTypeOpt        = new Option<string>("--type",         () => "feature", "Activity type: feature or fix");
var workNewTargetOpt      = new Option<string>("--target",       () => defaultTarget, "Base branch");
var workNewAssignedOpt    = new Option<string?>("--assigned-to", () => null, "Assigned-to email or display name");
var workNewTagsOpt        = new Option<string?>("--tags",        () => null, "Semicolon-separated tags");

var workNewCmd = new Command("new", "Create WI + branch + draft PR; switch to the new branch")
{ workNewTitleOpt, workNewDescOpt, workNewBranchOpt, workNewTypeOpt, workNewTargetOpt, workNewAssignedOpt, workNewTagsOpt, formatOpt };

workNewCmd.SetHandler(async (string title, string? description, string? branchSlug, string type, string target, string? assignedTo, string? tags, string format) =>
{
    var wiType = type.Equals("fix", StringComparison.OrdinalIgnoreCase) ? "Bug" : "User Story";
    var prefix = type.Equals("fix", StringComparison.OrdinalIgnoreCase) ? "fix" : "feature";

    using var http = CreateHttpClient();
    var repo = ResolveRepo();

    Console.Error.WriteLine($"[1/6] Ensuring WI ({wiType}) — '{title}'...");
    var wiId = await EnsureWi(http, title, wiType, description, tags, assignedTo);

    var slug = Slugify(branchSlug ?? title);
    var branchName = $"{prefix}/{wiId}-{slug}";

    Console.Error.WriteLine($"[2/6] Ensuring branch '{branchName}' (from '{target}')...");
    if (!await EnsureBranch(http, repo, branchName, target)) return;

    Console.Error.WriteLine($"[3/6] Ensuring draft PR...");
    var (prId, prJson) = await EnsurePr(http, repo, branchName, target, title, wiId, draft: true, description: $"Closes #{wiId}.");

    Console.Error.WriteLine($"[4/6] Linking branch + PR to WI...");
    var repoInfo = await GetRepoInfo(http, repo);
    if (repoInfo is not null)
    {
        var repoId    = repoInfo.Value.GetProperty("id").GetString()!;
        var projectId = repoInfo.Value.GetProperty("project").GetProperty("id").GetString()!;
        await EnsureWiLink(http, wiId, "ArtifactLink", BranchArtifactUri(projectId, repoId, branchName));
        await EnsureWiLink(http, wiId, "ArtifactLink", PrArtifactUri(projectId, repoId, prId));
    }

    Console.Error.WriteLine($"[5/6] Ensuring WI state Active...");
    await EnsureWiState(http, wiId, "Active");

    Console.Error.WriteLine($"[6/6] git fetch && git checkout {branchName}...");
    Git.Run("fetch", "origin");
    var (chkExit, _, chkErr) = Git.Run("checkout", branchName);
    if (chkExit != 0)
    {
        Console.Error.WriteLine($"git checkout failed: {chkErr}");
        Environment.ExitCode = 1;
        return;
    }

    var prState = prJson.TryGetProperty("status", out var st) ? st.GetString() ?? "draft" : "draft";
    var isDraft = prJson.TryGetProperty("isDraft", out var d) && d.GetBoolean();
    Render(format, new WorkActivityResult(
        wiId, title, wiType, "Active", branchName, prId,
        isDraft ? "draft" : prState, target, "— (no commits yet)", null, "started"));
}, workNewTitleOpt, workNewDescOpt, workNewBranchOpt, workNewTypeOpt, workNewTargetOpt, workNewAssignedOpt, workNewTagsOpt, formatOpt);

workCmd.AddCommand(workNewCmd);

// fm work load
var workLoadIdArg     = new Argument<string>("id", "WI id, branch name, or disambiguated id");
var workLoadTargetOpt = new Option<string>("--target", () => defaultTarget, "Base branch (used if branch/PR need creation)");

var workLoadCmd = new Command("load", "Resume an existing work item; repair + checkout") { workLoadIdArg, workLoadTargetOpt, formatOpt };

workLoadCmd.SetHandler(async (string idRaw, string target, string format) =>
{
    using var http = CreateHttpClient();
    var repo = ResolveRepo();

    var resolved = await ResolveId(http, repo, idRaw);
    if (resolved.WiId is null)
    {
        Console.Error.WriteLine($"Error: '{idRaw}' did not resolve to a WI.");
        Environment.ExitCode = 1;
        return;
    }
    var wiId = resolved.WiId.Value;

    var wi = await GetWi(http, wiId, expandRelations: true);
    if (wi is null) { Environment.ExitCode = 1; return; }
    var fields = wi.Value.GetProperty("fields");
    var title  = fields.TryGetProperty("System.Title", out var t) ? t.GetString() ?? "" : "";
    var state  = fields.TryGetProperty("System.State", out var s) ? s.GetString() ?? "" : "";
    var wiType = fields.TryGetProperty("System.WorkItemType", out var wt) ? wt.GetString() ?? "" : "";

    if (state is "Closed" or "Done" or "Removed")
    {
        int? mergedPrId = null;
        if (wi.Value.TryGetProperty("relations", out var rels))
        {
            foreach (var r in rels.EnumerateArray())
            {
                var url = r.TryGetProperty("url", out var u) ? u.GetString() ?? "" : "";
                var m = Regex.Match(url, @"PullRequestId/[^/]+%2F[^/]+%2F(\d+)");
                if (m.Success) { mergedPrId = int.Parse(m.Groups[1].Value); break; }
            }
        }
        Render(format, new WorkClosedResult(wiId, title, state, mergedPrId));
        return;
    }

    var prefix = wiType.Equals("Bug", StringComparison.OrdinalIgnoreCase) ? "fix" : "feature";
    var slug = Slugify(title);
    var branchName = $"{prefix}/{wiId}-{slug}";

    Console.Error.WriteLine($"[1/5] Ensuring branch '{branchName}'...");
    if (!await EnsureBranch(http, repo, branchName, target)) return;

    Console.Error.WriteLine($"[2/5] Ensuring draft PR...");
    var (prId, prJson) = await EnsurePr(http, repo, branchName, target, title, wiId, draft: true, description: $"Closes #{wiId}.");

    Console.Error.WriteLine($"[3/5] Repairing WI links...");
    var repoInfo = await GetRepoInfo(http, repo);
    if (repoInfo is not null)
    {
        var repoId    = repoInfo.Value.GetProperty("id").GetString()!;
        var projectId = repoInfo.Value.GetProperty("project").GetProperty("id").GetString()!;
        await EnsureWiLink(http, wiId, "ArtifactLink", BranchArtifactUri(projectId, repoId, branchName));
        await EnsureWiLink(http, wiId, "ArtifactLink", PrArtifactUri(projectId, repoId, prId));
    }

    Console.Error.WriteLine($"[4/5] Ensuring WI state Active...");
    await EnsureWiState(http, wiId, "Active");

    Console.Error.WriteLine($"[5/5] git fetch && git checkout {branchName}...");
    Git.Run("fetch", "origin");
    var (chkExit, _, chkErr) = Git.Run("checkout", branchName);
    if (chkExit != 0)
    {
        Console.Error.WriteLine($"git checkout failed: {chkErr}");
        Environment.ExitCode = 1;
        return;
    }

    string? stashRestored = null;
    var (_, stashList, _) = Git.Run("stash", "list");
    var stashName = $"stash-{wiId}-";
    var stashLine = stashList.Split('\n').FirstOrDefault(l => l.Contains(stashName));
    if (stashLine is not null)
    {
        var refMatch = Regex.Match(stashLine, @"^(stash@\{\d+\})");
        if (refMatch.Success)
        {
            var (popExit, _, popErr) = Git.Run("stash", "pop", refMatch.Groups[1].Value);
            stashRestored = popExit == 0 ? "restored" : $"conflict: {popErr.Trim()}";
        }
    }

    var prState = prJson.TryGetProperty("status", out var st) ? st.GetString() ?? "draft" : "draft";
    var isDraft = prJson.TryGetProperty("isDraft", out var d) && d.GetBoolean();
    Render(format, new WorkActivityResult(
        wiId, title, wiType, "Active", branchName, prId,
        isDraft ? "draft" : prState, target, "—", stashRestored, "loaded"));
}, workLoadIdArg, workLoadTargetOpt, formatOpt);

workCmd.AddCommand(workLoadCmd);

// fm work list
var workListMineOpt   = new Option<bool>  ("--mine",  "Filter by current user");
var workListStateOpt  = new Option<string>("--state", () => "Active", "Work item state");
var workListTypeOpt   = new Option<string>("--type",  () => "all", "Activity type: feature, fix, all");
var workListMaxOpt    = new Option<int>   ("--max",   () => 20, "Max results");

var workListCmd = new Command("list", "List active work items") { workListMineOpt, workListStateOpt, workListTypeOpt, workListMaxOpt, formatOpt };

workListCmd.SetHandler(async (bool mine, string state, string type, int max, string format) =>
{
    using var http = CreateHttpClient();
    var project = Environment.GetEnvironmentVariable("ADO_PROJECT")!;
    var sb = new StringBuilder($"SELECT [System.Id] FROM WorkItems WHERE [System.TeamProject] = '{project}'");
    if (!string.IsNullOrEmpty(state) && !state.Equals("all", StringComparison.OrdinalIgnoreCase))
        sb.Append($" AND [System.State] = '{state}'");
    if (type.Equals("feature", StringComparison.OrdinalIgnoreCase))
        sb.Append(" AND [System.WorkItemType] = 'User Story'");
    else if (type.Equals("fix", StringComparison.OrdinalIgnoreCase))
        sb.Append(" AND [System.WorkItemType] = 'Bug'");
    else
        sb.Append(" AND ([System.WorkItemType] = 'User Story' OR [System.WorkItemType] = 'Bug')");
    if (mine) sb.Append(" AND [System.AssignedTo] = @Me");
    sb.Append(" ORDER BY [System.ChangedDate] DESC");

    var ids = (await WiqlIds(http, sb.ToString(), max)).Take(max).ToList();
    var items = await GetWisBatch(http, ids);
    Render(format, new WorkListResult(items.Count, items));
}, workListMineOpt, workListStateOpt, workListTypeOpt, workListMaxOpt, formatOpt);

workCmd.AddCommand(workListCmd);

// ── PHASE_TASK_SUBCOMMANDS ────────────────────────────────────────────────────

// fm task hold
var holdStashOpt = new Option<bool>("--stash", "Stash uncommitted changes");
var holdForceOpt = new Option<bool>("--force", "Discard uncommitted changes (destructive)");
var holdStayOpt  = new Option<bool>("--stay",  "Stay on current branch after hold");

var taskHoldCmd = new Command("hold", "Pause activity, push, return to baseline") { holdStashOpt, holdForceOpt, holdStayOpt, formatOpt };

taskHoldCmd.SetHandler(async (bool stash, bool force, bool stay, string format) =>
{
    await Task.CompletedTask;
    var ctx = ParseBranchContext();
    if (!ctx.IsActivity)
    {
        Render(format, new TaskHoldResult(ctx.BranchName, null, null, "Already on baseline — nothing to hold."));
        return;
    }

    var dirty = !Git.IsClean();
    string? stashName = null;

    if (dirty && !stash && !force)
    {
        var (_, status, _) = Git.Run("status", "--short");
        Render(format, new ErrorResult("Hold blocked",
            $"Uncommitted changes present:\n\n```\n{status.TrimEnd()}\n```\n\nUse `--stash` to save them or `--force` to discard."));
        Environment.ExitCode = 1;
        return;
    }

    if (dirty && stash)
    {
        stashName = $"stash-{ctx.WorkItemId}-{ctx.Slug}";
        var (sExit, _, sErr) = Git.Run("stash", "push", "-u", "-m", stashName);
        if (sExit != 0) { Console.Error.WriteLine($"git stash failed: {sErr}"); Environment.ExitCode = 1; return; }
    }
    if (dirty && force)
    {
        Git.Run("checkout", "--", ".");
        Git.Run("clean", "-fd");
    }

    var (pExit, _, pErr) = Git.Run("push");
    if (pExit != 0) { Console.Error.WriteLine($"git push failed: {pErr}"); Environment.ExitCode = 1; return; }

    string nowOn = ctx.BranchName;
    if (!stay)
    {
        var (cExit, _, cErr) = Git.Run("checkout", defaultTarget);
        if (cExit != 0) { Console.Error.WriteLine($"git checkout {defaultTarget} failed: {cErr}"); Environment.ExitCode = 1; return; }
        nowOn = defaultTarget;
    }

    Render(format, new TaskHoldResult(ctx.BranchName, stashName, nowOn, null));
}, holdStashOpt, holdForceOpt, holdStayOpt, formatOpt);

taskCmd.AddCommand(taskHoldCmd);

// fm task update
var taskUpdateTitleOpt    = new Option<string?>("--title",        () => null, "New title");
var taskUpdateStateOpt    = new Option<string?>("--state",        () => null, "New state");
var taskUpdateDescOpt     = new Option<string?>("--description",  () => null, "New description");
var taskUpdateAssignedOpt = new Option<string?>("--assigned-to",  () => null, "New assigned-to");
var taskUpdateTagsOpt     = new Option<string?>("--tags",         () => null, "New tags");

var taskUpdateCmd = new Command("update", "Update the WI of the current activity") { taskUpdateTitleOpt, taskUpdateStateOpt, taskUpdateDescOpt, taskUpdateAssignedOpt, taskUpdateTagsOpt, formatOpt };

taskUpdateCmd.SetHandler(async (string? title, string? state, string? desc, string? assignedTo, string? tags, string format) =>
{
    var ctx = EnsureActivity(ParseBranchContext());
    using var http = CreateHttpClient();
    var updated = await PatchWi(http, ctx.WorkItemId!.Value,
        ("/fields/System.Title",       title),
        ("/fields/System.State",       state),
        ("/fields/System.Description", desc),
        ("/fields/System.AssignedTo",  assignedTo),
        ("/fields/System.Tags",        tags));
    if (updated is null) return;
    Render(format, new TaskUpdateResult(ctx.WorkItemId.Value, updated.Value));
}, taskUpdateTitleOpt, taskUpdateStateOpt, taskUpdateDescOpt, taskUpdateAssignedOpt, taskUpdateTagsOpt, formatOpt);

taskCmd.AddCommand(taskUpdateCmd);

// fm task complete
var taskCompleteCmd = new Command("complete", "Verify activity is done; return to baseline") { formatOpt };

taskCompleteCmd.SetHandler(async (string format) =>
{
    var ctx = ParseBranchContext();
    if (!ctx.IsActivity)
    {
        Render(format, new ErrorResult("Already on baseline", "Nothing to complete."));
        Environment.ExitCode = 1;
        return;
    }

    using var http = CreateHttpClient();
    var repo = ResolveRepo();
    var wi = await GetWi(http, ctx.WorkItemId!.Value);
    if (wi is null) { Environment.ExitCode = 1; return; }
    var wiState = wi.Value.GetProperty("fields").TryGetProperty("System.State", out var s) ? s.GetString() ?? "" : "";

    var prs = await GetPrsBySource(http, repo, ctx.BranchName, "all");
    var pr  = prs.FirstOrDefault();
    var prState = pr.ValueKind == JsonValueKind.Object ? pr.GetProperty("status").GetString() ?? "" : "—";
    var isDraft = pr.ValueKind == JsonValueKind.Object && pr.TryGetProperty("isDraft", out var d) && d.GetBoolean();
    var prId = pr.ValueKind == JsonValueKind.Object ? pr.GetProperty("pullRequestId").GetInt32() : 0;

    if (prState == "active" || isDraft)
    {
        Render(format, new ErrorResult("Cannot complete",
            $"PR #{prId} is `{(isDraft ? "draft" : "active")}` — merge or publish first.\n  WI #{ctx.WorkItemId} state: `{wiState}`."));
        Environment.ExitCode = 1;
        return;
    }

    var (cExit, _, cErr) = Git.Run("checkout", defaultTarget);
    if (cExit != 0) { Console.Error.WriteLine($"git checkout failed: {cErr}"); Environment.ExitCode = 1; return; }
    Git.Run("pull");

    Render(format, new TaskCompleteResult(ctx.WorkItemId.Value, wiState, prId, prState, defaultTarget));
}, formatOpt);

taskCmd.AddCommand(taskCompleteCmd);

// fm task sync
var syncRebaseOpt = new Option<bool>("--rebase", "Use rebase instead of merge");
var syncCheckOpt  = new Option<bool>("--check",  "Dry-run: show divergence only");

var taskSyncCmd = new Command("sync", "Sync activity branch with baseline") { syncRebaseOpt, syncCheckOpt, formatOpt };

taskSyncCmd.SetHandler(async (bool rebase, bool check, string format) =>
{
    await Task.CompletedTask;
    var ctx = EnsureActivity(ParseBranchContext());

    Git.Run("fetch", "origin");
    var (ahead, behind) = Git.AheadBehind($"origin/{defaultTarget}");

    if (check)
    {
        var (_, behindLog, _) = Git.Run("log", "--oneline", $"HEAD..origin/{defaultTarget}");
        var (_, aheadLog,  _) = Git.Run("log", "--oneline", $"origin/{defaultTarget}..HEAD");
        Render(format, new TaskSyncCheckResult(
            ctx.BranchName, defaultTarget, ahead, behind,
            behindLog.Split('\n', StringSplitOptions.RemoveEmptyEntries).ToList(),
            aheadLog.Split('\n', StringSplitOptions.RemoveEmptyEntries).ToList()));
        return;
    }

    if (behind == 0)
    {
        Render(format, new TaskSyncResult(ctx.BranchName, rebase ? "rebase" : "merge", defaultTarget, behind, true, new()));
        return;
    }

    var (mergedBefore, beforeLog, _) = Git.Run("log", "--oneline", $"HEAD..origin/{defaultTarget}");
    _ = mergedBefore;
    var commits = beforeLog.Split('\n', StringSplitOptions.RemoveEmptyEntries).ToList();

    var (mExit, mOut, mErr) = rebase
        ? Git.Run("rebase", $"origin/{defaultTarget}")
        : Git.Run("merge",  $"origin/{defaultTarget}");

    if (mExit != 0)
    {
        var (_, statusOut, _) = Git.Run("status", "--short");
        var conflicting = statusOut.Split('\n', StringSplitOptions.RemoveEmptyEntries)
            .Where(l => l.StartsWith("UU ") || l.StartsWith("AA ") || l.StartsWith("DD ") || l.Contains("both modified"))
            .Select(l => l.Length > 3 ? l[3..].Trim() : l.Trim())
            .ToList();
        Console.Error.WriteLine(mOut);
        Console.Error.WriteLine(mErr);
        Render(format, new TaskSyncConflictResult(ctx.BranchName, rebase ? "rebase" : "merge", defaultTarget, conflicting));
        Environment.ExitCode = 1;
        return;
    }

    Git.Run("push");
    Render(format, new TaskSyncResult(ctx.BranchName, rebase ? "rebase" : "merge", defaultTarget, behind, true, commits));
}, syncRebaseOpt, syncCheckOpt, formatOpt);

taskCmd.AddCommand(taskSyncCmd);

// ── PHASE_PR_SUBCOMMANDS ──────────────────────────────────────────────────────

// fm pr show
var prShowIdArg = new Argument<string?>("id", () => null, "PR id, WI id, or branch (defaults to current branch)");

var prShowCmd = new Command("show", "Show PR details") { prShowIdArg, formatOpt };

prShowCmd.SetHandler(async (string? idRaw, string format) =>
{
    using var http = CreateHttpClient();
    var repo = ResolveRepo();

    int? prId = null;
    if (idRaw is not null)
    {
        var resolved = await ResolveId(http, repo, idRaw);
        if (resolved.PrId.HasValue) prId = resolved.PrId;
        else if (resolved.WiId.HasValue)
        {
            var wi = await GetWi(http, resolved.WiId.Value, expandRelations: true);
            if (wi is not null && wi.Value.TryGetProperty("relations", out var rels))
            {
                foreach (var r in rels.EnumerateArray())
                {
                    var url = r.TryGetProperty("url", out var u) ? u.GetString() ?? "" : "";
                    var m = Regex.Match(url, @"PullRequestId/[^/]+%2F[^/]+%2F(\d+)");
                    if (m.Success) { prId = int.Parse(m.Groups[1].Value); break; }
                }
            }
        }
    }
    else
    {
        var ctx = EnsureActivity(ParseBranchContext());
        var prs = await GetPrsBySource(http, repo, ctx.BranchName, "active");
        if (prs.Count > 0) prId = prs[0].GetProperty("pullRequestId").GetInt32();
    }

    if (prId is null)
    {
        Render(format, new ErrorResult("PR not found", "No PR matched the given id/branch."));
        Environment.ExitCode = 1;
        return;
    }

    var pr = await GetPr(http, repo, prId.Value);
    if (pr is null) { Environment.ExitCode = 1; return; }
    Render(format, new PrShowResult(prId.Value, pr.Value));
}, prShowIdArg, formatOpt);

prCmd.AddCommand(prShowCmd);

// fm pr update
var prUpdateTitleOpt    = new Option<string?>("--title",        () => null, "New title");
var prUpdateDescOpt     = new Option<string?>("--description",  () => null, "New description");
var prUpdatePublishOpt  = new Option<bool>   ("--publish",      "Remove draft status");
var prUpdateStatusOpt   = new Option<string?>("--status",       () => null, "active, abandoned, completed");
var prUpdateReviewerOpt = new Option<string?>("--add-reviewer", () => null, "Add a reviewer (email)");

var prUpdateCmd = new Command("update", "Update PR linked to current activity")
{ prUpdateTitleOpt, prUpdateDescOpt, prUpdatePublishOpt, prUpdateStatusOpt, prUpdateReviewerOpt, formatOpt };

prUpdateCmd.SetHandler(async (string? title, string? desc, bool publish, string? status, string? addReviewer, string format) =>
{
    var ctx = EnsureActivity(ParseBranchContext());
    using var http = CreateHttpClient();
    var repo = ResolveRepo();

    var prs = await GetPrsBySource(http, repo, ctx.BranchName, "active");
    if (prs.Count == 0)
    {
        Render(format, new ErrorResult("No PR", $"No active PR for branch `{ctx.BranchName}`."));
        Environment.ExitCode = 1;
        return;
    }
    var prId = prs[0].GetProperty("pullRequestId").GetInt32();

    var body = new JsonObject();
    if (title  is not null) body["title"]       = title;
    if (desc   is not null) body["description"] = desc;
    if (status is not null) body["status"]      = status;
    if (publish)            body["isDraft"]     = false;

    JsonElement? updated = null;
    if (body.Count > 0)
    {
        updated = await PatchPr(http, repo, prId, body);
        if (updated is null) return;
    }
    else
    {
        updated = await GetPr(http, repo, prId);
    }

    if (addReviewer is not null)
    {
        var revBody = new JsonObject { ["vote"] = 0 };
        var req = new HttpRequestMessage(HttpMethod.Put,
            V($"git/repositories/{Uri.EscapeDataString(repo)}/pullrequests/{prId}/reviewers/{Uri.EscapeDataString(addReviewer)}"))
        {
            Content = new StringContent(revBody.ToJsonString(), Encoding.UTF8, "application/json")
        };
        var resp = await http.SendAsync(req);
        if (!resp.IsSuccessStatusCode)
            Console.Error.WriteLine($"Warning: could not add reviewer '{addReviewer}': {resp.StatusCode}");
    }

    Render(format, new PrShowResult(prId, updated!.Value));
}, prUpdateTitleOpt, prUpdateDescOpt, prUpdatePublishOpt, prUpdateStatusOpt, prUpdateReviewerOpt, formatOpt);

prCmd.AddCommand(prUpdateCmd);

// fm pr merge
var prMergeStrategyOpt = new Option<string?>("--strategy",            () => null, "squash, rebase, rebaseMerge, noFastForward");
var prMergeDeleteOpt   = new Option<bool>   ("--delete-source-branch", "Delete source branch after merge");
var prMergeBypassOpt   = new Option<bool>   ("--bypass-policy",        "Bypass branch policies");

var prMergeCmd = new Command("merge", "Complete (merge) PR for current activity")
{ prMergeStrategyOpt, prMergeDeleteOpt, prMergeBypassOpt, formatOpt };

prMergeCmd.SetHandler(async (string? strategyArg, bool deleteSource, bool bypass, string format) =>
{
    var ctx = EnsureActivity(ParseBranchContext());
    using var http = CreateHttpClient();
    var repo = ResolveRepo();

    var prs = await GetPrsBySource(http, repo, ctx.BranchName, "active");
    if (prs.Count == 0)
    {
        Render(format, new ErrorResult("No PR", $"No active PR for branch `{ctx.BranchName}`."));
        Environment.ExitCode = 1;
        return;
    }
    var prId = prs[0].GetProperty("pullRequestId").GetInt32();
    var pr = await GetPr(http, repo, prId);
    if (pr is null) { Environment.ExitCode = 1; return; }

    var isDraft = pr.Value.TryGetProperty("isDraft", out var d) && d.GetBoolean();
    if (isDraft)
    {
        Render(format, new ErrorResult("PR is draft", $"PR #{prId} is draft — publish first with `fm pr update --publish`."));
        Environment.ExitCode = 1;
        return;
    }

    var mergeStatus = pr.Value.TryGetProperty("mergeStatus", out var ms) ? ms.GetString() ?? "" : "";
    if (mergeStatus is not "succeeded" and not "")
    {
        var failures = new List<string> { $"merge status: {mergeStatus}" };
        Render(format, new PrMergeErrorResult(prId, failures));
        Environment.ExitCode = 1;
        return;
    }

    var strategy = strategyArg ?? defaultMergeStrat;
    var body = new JsonObject
    {
        ["status"] = "completed",
        ["lastMergeSourceCommit"] = new JsonObject { ["commitId"] = pr.Value.GetProperty("lastMergeSourceCommit").GetProperty("commitId").GetString() },
        ["completionOptions"] = new JsonObject
        {
            ["mergeStrategy"]      = strategy,
            ["deleteSourceBranch"] = deleteSource,
            ["bypassPolicy"]       = bypass
        }
    };
    var merged = await PatchPr(http, repo, prId, body);
    if (merged is null) return;

    var commit = merged.Value.TryGetProperty("lastMergeCommit", out var lmc) && lmc.TryGetProperty("commitId", out var cid)
        ? cid.GetString() ?? "—" : "—";
    var target = StripRefsHeads(merged.Value.GetProperty("targetRefName").GetString() ?? "");

    await EnsureWiState(http, ctx.WorkItemId!.Value, "Closed");

    Render(format, new PrMergeResult(prId, strategy, ctx.WorkItemId.Value, target, commit.Length > 7 ? commit[..7] : commit));
}, prMergeStrategyOpt, prMergeDeleteOpt, prMergeBypassOpt, formatOpt);

prCmd.AddCommand(prMergeCmd);

// fm pr review
var prReviewIdArg = new Argument<string>("id", "PR id, WI id, or branch");

var prReviewCmd = new Command("review", "Switch to another PR's branch for review") { prReviewIdArg, formatOpt };

prReviewCmd.SetHandler(async (string idRaw, string format) =>
{
    using var http = CreateHttpClient();
    var repo = ResolveRepo();

    var ctx = ParseBranchContext();
    string? stashName = null;
    if (ctx.IsActivity)
    {
        if (!Git.IsClean())
        {
            stashName = $"stash-{ctx.WorkItemId}-{ctx.Slug}";
            var (sExit, _, sErr) = Git.Run("stash", "push", "-u", "-m", stashName);
            if (sExit != 0) { Console.Error.WriteLine($"git stash failed: {sErr}"); Environment.ExitCode = 1; return; }
        }
        Git.Run("push");
    }

    var resolved = await ResolveId(http, repo, idRaw);
    int? prId = resolved.PrId;
    if (prId is null && resolved.WiId is not null)
    {
        var wi = await GetWi(http, resolved.WiId.Value, expandRelations: true);
        if (wi is not null && wi.Value.TryGetProperty("relations", out var rels))
        {
            foreach (var r in rels.EnumerateArray())
            {
                var url = r.TryGetProperty("url", out var u) ? u.GetString() ?? "" : "";
                var m = Regex.Match(url, @"PullRequestId/[^/]+%2F[^/]+%2F(\d+)");
                if (m.Success) { prId = int.Parse(m.Groups[1].Value); break; }
            }
        }
    }
    if (prId is null)
    {
        Render(format, new ErrorResult("PR not found", $"Cannot resolve `{idRaw}` to a PR."));
        Environment.ExitCode = 1;
        return;
    }

    var pr = await GetPr(http, repo, prId.Value);
    if (pr is null) { Environment.ExitCode = 1; return; }
    var sourceBranch = StripRefsHeads(pr.Value.GetProperty("sourceRefName").GetString() ?? "");

    Git.Run("fetch", "origin");
    var (cExit, _, cErr) = Git.Run("checkout", sourceBranch);
    if (cExit != 0) { Console.Error.WriteLine($"git checkout failed: {cErr}"); Environment.ExitCode = 1; return; }

    Render(format, new PrReviewResult(prId.Value, sourceBranch, ctx.IsActivity ? ctx.BranchName : null, stashName));
}, prReviewIdArg, formatOpt);

prCmd.AddCommand(prReviewCmd);

// ── PHASE_PIPELINE_SUBCOMMANDS ────────────────────────────────────────────────

// fm pipeline run
var pipelineRunIdOpt = new Option<int?>("--id", () => null, "Pipeline definition id");

var pipelineRunCmd = new Command("run", "Trigger a CI pipeline for the current branch") { pipelineRunIdOpt, formatOpt };

pipelineRunCmd.SetHandler(async (int? pipelineId, string format) =>
{
    var ctx = ParseBranchContext();
    var branch = ctx.BranchName;

    using var http = CreateHttpClient();

    if (pipelineId is null)
    {
        var resp = await http.GetAsync(V("pipelines?$top=500"));
        if (await ExitOnError(resp) != 0) return;
        using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
        var pipelines = doc.RootElement.GetProperty("value").EnumerateArray().Select(e => e.Clone()).ToList();
        Render(format, new PipelineListResult(pipelines.Count, pipelines, "--id required — pick one and re-run"));
        Environment.ExitCode = 1;
        return;
    }

    var bodyObj = new JsonObject
    {
        ["resources"] = new JsonObject
        {
            ["repositories"] = new JsonObject
            {
                ["self"] = new JsonObject { ["refName"] = NormalizeRef(branch) }
            }
        }
    };

    var triggerResp = await http.PostAsync(V($"pipelines/{pipelineId}/runs"),
        new StringContent(bodyObj.ToJsonString(), Encoding.UTF8, "application/json"));
    if (await ExitOnError(triggerResp) != 0) return;
    using var rDoc = await JsonDocument.ParseAsync(await triggerResp.Content.ReadAsStreamAsync());
    var runId = rDoc.RootElement.GetProperty("id").GetInt32();
    Render(format, new PipelineRunResult(pipelineId.Value, runId, branch, rDoc.RootElement.Clone()));
}, pipelineRunIdOpt, formatOpt);

pipelineCmd.AddCommand(pipelineRunCmd);

// fm pipeline status
var pipelineStatusRunIdOpt = new Option<int?>("--run-id", () => null, "Build run id");
var pipelineStatusWatchOpt = new Option<bool>("--watch",                "Poll every 30s until completed");

var pipelineStatusCmd = new Command("status", "Show latest CI run status for current branch") { pipelineStatusRunIdOpt, pipelineStatusWatchOpt, formatOpt };

pipelineStatusCmd.SetHandler(async (int? runId, bool watch, string format) =>
{
    var ctx = ParseBranchContext();
    using var http = CreateHttpClient();

    while (true)
    {
        JsonElement run;
        if (runId.HasValue)
        {
            var resp = await http.GetAsync(V($"build/builds/{runId}"));
            if (await ExitOnError(resp) != 0) return;
            using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
            run = doc.RootElement.Clone();
        }
        else
        {
            var resp = await http.GetAsync(V($"build/builds?branchName={Uri.EscapeDataString(NormalizeRef(ctx.BranchName))}&$top=1"));
            if (await ExitOnError(resp) != 0) return;
            using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
            var arr = doc.RootElement.GetProperty("value");
            if (arr.GetArrayLength() == 0)
            {
                Render(format, new ErrorResult("No CI runs", $"No builds found for branch `{ctx.BranchName}`."));
                Environment.ExitCode = 1;
                return;
            }
            run = arr[0].Clone();
        }

        var status = run.TryGetProperty("status", out var s) ? s.GetString() ?? "" : "";
        Render(format, new PipelineStatusResult(
            run.GetProperty("id").GetInt32(),
            run.TryGetProperty("definition", out var def) && def.TryGetProperty("name", out var dn) ? dn.GetString() : null,
            status,
            run.TryGetProperty("result", out var rs) ? rs.GetString() : null,
            run.TryGetProperty("sourceBranch", out var sb) ? StripRefsHeads(sb.GetString() ?? "") : ctx.BranchName,
            run));

        if (!watch || status == "completed") return;
        await Task.Delay(TimeSpan.FromSeconds(30));
    }
}, pipelineStatusRunIdOpt, pipelineStatusWatchOpt, formatOpt);

pipelineCmd.AddCommand(pipelineStatusCmd);

// ── PHASE_TODO_SUBCOMMANDS ────────────────────────────────────────────────────
// Phase 8 attaches todo/* subcommands here.

// ── PHASE_LEAF_COMMANDS ───────────────────────────────────────────────────────

// fm context
var ctxOnlyWiOpt       = new Option<bool>("--only-wi",       "Show only work item details");
var ctxOnlyPrOpt       = new Option<bool>("--only-pr",       "Show only PR details");
var ctxOnlyGitOpt      = new Option<bool>("--only-git",      "Show only git status");
var ctxOnlyPipelineOpt = new Option<bool>("--only-pipeline", "Show only latest CI run");

var contextCmd = new Command("context", "Show current activity context")
{ ctxOnlyWiOpt, ctxOnlyPrOpt, ctxOnlyGitOpt, ctxOnlyPipelineOpt, formatOpt };

contextCmd.SetHandler(async (bool onlyWi, bool onlyPr, bool onlyGit, bool onlyPipeline, string format) =>
{
    var ctx = ParseBranchContext();
    if (!ctx.IsActivity)
    {
        var (_, log, _) = Git.Run("log", "--oneline", "-5");
        var commits = log.Split('\n', StringSplitOptions.RemoveEmptyEntries).ToList();
        Render(format, new ContextBaselineResult(ctx.BranchName, commits));
        return;
    }

    using var http = CreateHttpClient();
    var repo = ResolveRepo();
    var wiId = ctx.WorkItemId!.Value;

    JsonElement? wi = null;
    JsonElement? pr = null;
    int? prId = null;
    JsonElement? buildRun = null;
    string? buildPipeline = null;
    int? buildId = null;
    int ahead = 0, behind = 0;
    bool clean = true;

    if (!onlyPr && !onlyGit && !onlyPipeline)
        wi = await GetWi(http, wiId);

    if (!onlyWi && !onlyGit && !onlyPipeline)
    {
        var prs = await GetPrsBySource(http, repo, ctx.BranchName, "active");
        if (prs.Count > 0) { pr = prs[0]; prId = pr.Value.GetProperty("pullRequestId").GetInt32(); }
    }

    if (!onlyWi && !onlyPr && !onlyPipeline)
    {
        Git.Run("fetch", "origin", "--quiet");
        (ahead, behind) = Git.AheadBehind($"origin/{defaultTarget}");
        clean = Git.IsClean();
    }

    if (!onlyWi && !onlyPr && !onlyGit)
    {
        var resp = await http.GetAsync(V($"build/builds?branchName={Uri.EscapeDataString(NormalizeRef(ctx.BranchName))}&$top=1"));
        if (resp.IsSuccessStatusCode)
        {
            using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
            var arr = doc.RootElement.GetProperty("value");
            if (arr.GetArrayLength() > 0)
            {
                buildRun = arr[0].Clone();
                if (buildRun.Value.TryGetProperty("definition", out var def))
                {
                    buildPipeline = def.TryGetProperty("name", out var dn) ? dn.GetString() : null;
                    buildId       = def.TryGetProperty("id",   out var di) ? di.GetInt32() : null;
                }
            }
        }
    }

    var todos = new List<JsonElement>();
    if (!onlyWi && !onlyPr && !onlyGit && !onlyPipeline)
    {
        var childIds = await WiqlIds(http,
            $"SELECT [System.Id] FROM WorkItemLinks WHERE [Source].[System.Id] = {wiId} AND [System.Links.LinkType] = 'System.LinkTypes.Hierarchy-Forward' MODE (Recursive)", 200);
        childIds = childIds.Where(i => i != wiId).ToList();
        if (childIds.Count > 0)
            todos = await GetWisBatch(http, childIds);
    }

    Render(format, new ContextActivityResult(
        ctx.BranchName, wi, prId, pr, ahead, behind, clean,
        buildPipeline, buildId, buildRun, todos,
        onlyWi, onlyPr, onlyGit, onlyPipeline, defaultTarget));
}, ctxOnlyWiOpt, ctxOnlyPrOpt, ctxOnlyGitOpt, ctxOnlyPipelineOpt, formatOpt);

// PHASE_ROOT_LEAVES_INSERT — commit/push/sync go here in Phase 9.

// ── root ──────────────────────────────────────────────────────────────────────
var rootCmd = new RootCommand("Flow Manager — porcelain commands for daily ADO + git workflow")
{
    workCmd, taskCmd, prCmd, pipelineCmd, todoCmd
};
rootCmd.AddCommand(contextCmd);
// PHASE_ROOT_LEAVES — Phase 9 will add commit, push, sync here.
return await rootCmd.InvokeAsync(args);

// ─── local functions ──────────────────────────────────────────────────────────

void Render(string format, object data)
{
    var output = format.ToLowerInvariant() switch
    {
        "yaml"     => yaml.Serialize(ToYamlObject(data)),
        "markdown" => RenderMarkdown(data),
        _          => JsonSerializer.Serialize(data, jsonOpts)
    };
    Console.WriteLine(output);
}

string RenderMarkdown(object data) => data switch
{
    WorkActivityResult     r => RenderWorkActivity(r),
    WorkClosedResult       r => RenderWorkClosed(r),
    WorkListResult         r => RenderWorkList(r),
    ContextBaselineResult  r => RenderContextBaseline(r),
    ContextActivityResult  r => RenderContextActivity(r),
    TaskHoldResult         r => RenderTaskHold(r),
    TaskUpdateResult       r => RenderTaskUpdate(r),
    TaskCompleteResult     r => RenderTaskComplete(r),
    TaskSyncResult         r => RenderTaskSync(r),
    TaskSyncCheckResult    r => RenderTaskSyncCheck(r),
    TaskSyncConflictResult r => RenderTaskSyncConflict(r),
    PrShowResult           r => RenderPrShow(r),
    PrMergeResult          r => RenderPrMerge(r),
    PrMergeErrorResult     r => RenderPrMergeError(r),
    PrReviewResult         r => RenderPrReview(r),
    ErrorResult            r => $"## {r.Title}\n\n{r.Message}\n",
    // PHASE_RENDER_MARKER
    _ => throw new NotSupportedException($"No markdown renderer for {data.GetType().Name}")
};

object? ToYamlObject(object? value) => value switch
{
    JsonElement el => el.ValueKind switch
    {
        JsonValueKind.Object  => el.EnumerateObject().ToDictionary(p => p.Name, p => ToYamlObject(p.Value)),
        JsonValueKind.Array   => el.EnumerateArray().Select(e => ToYamlObject(e)).ToList(),
        JsonValueKind.String  => el.GetString(),
        JsonValueKind.Number  => el.TryGetInt64(out var l) ? (object)l : el.GetDouble(),
        JsonValueKind.True    => true,
        JsonValueKind.False   => false,
        _                     => null
    },
    // PHASE_YAML_MARKER
    _ => value
};

// ── ADO REST helpers (mirrors ado.cs) ─────────────────────────────────────────

string V(string url) => url.Contains('?') ? $"{url}&api-version=7.1" : $"{url}?api-version=7.1";

string NormalizeRef(string branch)
    => branch.StartsWith("refs/", StringComparison.OrdinalIgnoreCase) ? branch : $"refs/heads/{branch}";

string StripRefsHeads(string refName)
    => refName.StartsWith("refs/heads/", StringComparison.OrdinalIgnoreCase) ? refName["refs/heads/".Length..] : refName;

string ExtractString(JsonElement v) => v.ValueKind switch
{
    JsonValueKind.Null   => "—",
    JsonValueKind.Object => v.TryGetProperty("displayName", out var dn) ? dn.GetString() ?? "—" : "—",
    _                    => v.GetString() ?? "—"
};

StringContent BuildPatch(params (string path, object? value)[] ops)
{
    var arr = new JsonArray();
    foreach (var (path, value) in ops)
    {
        if (value is null) continue;
        arr.Add(new JsonObject
        {
            ["op"]    = "add",
            ["path"]  = path,
            ["value"] = value.ToString()
        });
    }
    return new StringContent(arr.ToJsonString(), Encoding.UTF8, "application/json-patch+json");
}

HttpClient CreateHttpClient()
{
    var adoUrl  = Environment.GetEnvironmentVariable("ADO_URL")?.TrimEnd('/')
        ?? throw new InvalidOperationException("ADO_URL environment variable is not set.");
    var project = Environment.GetEnvironmentVariable("ADO_PROJECT")
        ?? throw new InvalidOperationException("ADO_PROJECT environment variable is not set.");
    var pat     = Environment.GetEnvironmentVariable("ADO_PAT")
        ?? throw new InvalidOperationException("ADO_PAT environment variable is not set.");

    var baseAddress = $"{adoUrl}/{Uri.EscapeDataString(project)}/_apis/";
    var http = new HttpClient { BaseAddress = new Uri(baseAddress) };
    var encoded = Convert.ToBase64String(Encoding.UTF8.GetBytes($":{pat}"));
    http.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Basic", encoded);
    return http;
}

async Task<int> ExitOnError(HttpResponseMessage resp)
{
    if (!resp.IsSuccessStatusCode)
    {
        var body = await resp.Content.ReadAsStringAsync();
        Console.Error.WriteLine($"Error {(int)resp.StatusCode}: {body}");
        Environment.ExitCode = 1;
        return 1;
    }
    return 0;
}

// ── repo + branch context ─────────────────────────────────────────────────────

string ResolveRepo()
{
    var envRepo = Environment.GetEnvironmentVariable("FM_REPO");
    if (!string.IsNullOrWhiteSpace(envRepo)) return envRepo;

    var (exit, stdout, _) = Git.Run("remote", "get-url", "origin");
    if (exit != 0)
        throw new InvalidOperationException("Cannot determine ADO repo: not a git repo or no `origin` remote. Set FM_REPO.");

    var url = stdout.Trim();
    var httpsMatch = Regex.Match(url, @"dev\.azure\.com/[^/]+/[^/]+/_git/([^/?#]+)");
    if (httpsMatch.Success) return Uri.UnescapeDataString(httpsMatch.Groups[1].Value);

    var sshMatch = Regex.Match(url, @"ssh\.dev\.azure\.com[:/]v3/[^/]+/[^/]+/([^/?#]+)");
    if (sshMatch.Success) return Uri.UnescapeDataString(sshMatch.Groups[1].Value);

    throw new InvalidOperationException($"Cannot parse ADO repo from origin URL '{url}'. Set FM_REPO.");
}

BranchContext ParseBranchContext()
{
    var (exit, stdout, _) = Git.Run("rev-parse", "--abbrev-ref", "HEAD");
    if (exit != 0)
        throw new InvalidOperationException("Cannot read current git branch.");
    var name = stdout.Trim();

    var match = Regex.Match(name, @"^(feature|fix)/(\d+)-(.+)$");
    if (match.Success)
    {
        return new BranchContext(
            IsActivity: true,
            BranchName: name,
            ActivityType: match.Groups[1].Value,
            WorkItemId: int.Parse(match.Groups[2].Value),
            Slug: match.Groups[3].Value);
    }
    return new BranchContext(false, name, null, null, null);
}

BranchContext EnsureActivity(BranchContext ctx)
{
    if (!ctx.IsActivity)
    {
        Console.Error.WriteLine($"Error: command requires Activity context (current branch: {ctx.BranchName}).");
        Console.WriteLine($"## Error\n\n  Current branch `{ctx.BranchName}` is not an Activity branch.\n  Run `fm work new` or `fm work load` first.");
        Environment.Exit(1);
    }
    return ctx;
}

string Slugify(string title)
{
    var slug = Regex.Replace(title.ToLowerInvariant(), @"[^a-z0-9]+", "-").Trim('-');
    if (slug.Length > 40) slug = slug[..40].TrimEnd('-');
    return slug;
}

// ── PHASE_LOCAL_FUNCTIONS ─────────────────────────────────────────────────────
// Phases 2-9 add helper local functions (EnsureWi, EnsurePr, ResolveId, etc.) here.

// ── markdown renderers ────────────────────────────────────────────────────────

string RenderWorkActivity(WorkActivityResult r)
{
    var sb = new StringBuilder();
    var heading = r.Action switch
    {
        "started" => "New Activity Started",
        "loaded"  => "Activity Loaded",
        _         => "Activity"
    };
    sb.AppendLine($"## {heading}");
    sb.AppendLine();
    sb.AppendLine("| | |");
    sb.AppendLine("|-|---|");
    sb.AppendLine($"| Work Item | #{r.Id} — {r.Title} |");
    sb.AppendLine($"| Type      | {r.Type} |");
    sb.AppendLine($"| State     | {r.State} |");
    sb.AppendLine($"| Branch    | `{r.Branch}` |");
    sb.AppendLine($"| PR        | #{r.PrId} ({r.PrState}) |");
    sb.AppendLine($"| Mergeable | {r.Mergeable} |");
    sb.AppendLine($"| Target    | `{r.Target}` |");
    if (r.StashRestored is not null)
        sb.AppendLine($"| Stash restored | {r.StashRestored} |");
    return sb.ToString();
}

string RenderWorkClosed(WorkClosedResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine($"## Work Item #{r.Id} — {r.State}");
    sb.AppendLine();
    sb.AppendLine($"  Title  {r.Title}");
    sb.AppendLine($"  State  {r.State}");
    if (r.MergedPrId.HasValue)
        sb.AppendLine($"  PR     #{r.MergedPrId.Value} (merged)");
    sb.AppendLine();
    sb.AppendLine("  No branch switch — WI is closed.");
    return sb.ToString();
}

string RenderWorkList(WorkListResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine($"# Work Items ({r.Count})");
    sb.AppendLine();
    if (r.Count == 0)
    {
        sb.AppendLine("_No matching work items._");
        return sb.ToString();
    }
    sb.AppendLine("| ID | Type | State | Title | Assigned To |");
    sb.AppendLine("|----|------|-------|-------|-------------|");
    foreach (var item in r.Items)
    {
        var id = item.GetProperty("id").GetInt32();
        var f  = item.GetProperty("fields");
        string Field(string n) => f.TryGetProperty(n, out var v) ? ExtractString(v) : "—";
        sb.AppendLine($"| {id} | {Field("System.WorkItemType")} | {Field("System.State")} | {Field("System.Title")} | {Field("System.AssignedTo")} |");
    }
    return sb.ToString();
}

string RenderContextBaseline(ContextBaselineResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine($"## Context — `{r.Branch}` (baseline)");
    sb.AppendLine();
    sb.AppendLine("Last commits:");
    foreach (var c in r.RecentCommits) sb.AppendLine($"- {c}");
    return sb.ToString();
}

string RenderContextActivity(ContextActivityResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine($"## Context — `{r.Branch}`");
    sb.AppendLine();

    if (!r.OnlyPr && !r.OnlyGit && !r.OnlyPipeline && r.Wi.HasValue)
    {
        var f = r.Wi.Value.GetProperty("fields");
        string F(string n) => f.TryGetProperty(n, out var v) ? ExtractString(v) : "—";
        sb.AppendLine("### Work Item");
        sb.AppendLine("| | |");
        sb.AppendLine("|-|---|");
        sb.AppendLine($"| ID    | #{r.Wi.Value.GetProperty("id").GetInt32()} |");
        sb.AppendLine($"| Title | {F("System.Title")} |");
        sb.AppendLine($"| State | {F("System.State")} |");
        sb.AppendLine($"| Assigned | {F("System.AssignedTo")} |");
        sb.AppendLine();
    }

    if (!r.OnlyWi && !r.OnlyGit && !r.OnlyPipeline)
    {
        sb.AppendLine("### Pull Request");
        if (r.Pr.HasValue)
        {
            string PF(string n) => r.Pr.Value.TryGetProperty(n, out var v) ? ExtractString(v) : "—";
            var isDraft = r.Pr.Value.TryGetProperty("isDraft", out var d) && d.GetBoolean();
            var status  = isDraft ? "draft" : PF("status");
            var mergeStatus = r.Pr.Value.TryGetProperty("mergeStatus", out var ms) ? ms.GetString() : "—";
            sb.AppendLine("| | |");
            sb.AppendLine("|-|---|");
            sb.AppendLine($"| PR        | #{r.PrId} |");
            sb.AppendLine($"| State     | {status} |");
            sb.AppendLine($"| Mergeable | {mergeStatus} |");
            sb.AppendLine($"| Target    | `{StripRefsHeads(PF("targetRefName"))}` |");
        }
        else
        {
            sb.AppendLine("_No active PR for this branch._");
        }
        sb.AppendLine();
    }

    if (!r.OnlyWi && !r.OnlyPr && !r.OnlyPipeline)
    {
        sb.AppendLine("### Git");
        sb.AppendLine("| | |");
        sb.AppendLine("|-|---|");
        sb.AppendLine($"| Ahead  | {r.Ahead} commits |");
        sb.AppendLine($"| Behind | {r.Behind} commits |");
        sb.AppendLine($"| Local  | {(r.Clean ? "clean" : "dirty")} |");
        sb.AppendLine();
    }

    if (!r.OnlyWi && !r.OnlyPr && !r.OnlyGit)
    {
        sb.AppendLine("### CI");
        if (r.LatestRun.HasValue)
        {
            string RF(string n) => r.LatestRun.Value.TryGetProperty(n, out var v) ? ExtractString(v) : "—";
            sb.AppendLine("| | |");
            sb.AppendLine("|-|---|");
            sb.AppendLine($"| Pipeline | {r.PipelineName ?? "—"} (#{r.PipelineId?.ToString() ?? "—"}) |");
            var status = RF("status");
            var result = RF("result");
            var displayStatus = status == "completed" ? result : status;
            sb.AppendLine($"| Last run | #{RF("id")} — {displayStatus} |");
        }
        else
        {
            sb.AppendLine("_No CI runs found for this branch._");
        }
        sb.AppendLine();
    }

    if (!r.OnlyWi && !r.OnlyPr && !r.OnlyGit && !r.OnlyPipeline && r.Todos.Count > 0)
    {
        sb.AppendLine("### Todos");
        var open   = r.Todos.Where(t => GetField(t, "System.State") is "New" or "Active").ToList();
        var active = open.Count(t => GetField(t, "System.State") == "Active");
        var done   = r.Todos.Count(t => GetField(t, "System.State") is "Closed" or "Done" or "Resolved");
        var newCt  = open.Count - active;
        foreach (var t in open.OrderBy(t => GetField(t, "System.State") == "Active" ? 0 : 1).ThenBy(t => t.GetProperty("id").GetInt32()))
        {
            var id    = t.GetProperty("id").GetInt32();
            var title = GetField(t, "System.Title");
            var state = GetField(t, "System.State");
            var glyph = state == "Active" ? "●" : "○";
            sb.AppendLine($"  {glyph}  #{id}  {title}".PadRight(60) + (state == "Active" ? "Active" : ""));
        }
        sb.AppendLine("  ─────────────────────────────────────────");
        sb.AppendLine($"  {done} done · {active} active · {newCt} open · {r.Todos.Count} total  (run `fm todo show` for detail)");
    }

    return sb.ToString();
}

string GetField(JsonElement item, string name)
    => item.TryGetProperty("fields", out var f) && f.TryGetProperty(name, out var v) ? ExtractString(v) : "—";

string RenderTaskHold(TaskHoldResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine("## Task Hold");
    sb.AppendLine();
    if (r.Note is not null)
    {
        sb.AppendLine($"  {r.Note}");
        return sb.ToString();
    }
    sb.AppendLine("| | |");
    sb.AppendLine("|-|---|");
    sb.AppendLine($"| Branch pushed | `{r.Branch}` |");
    sb.AppendLine($"| Stash         | {(r.Stash is null ? "—" : $"`{r.Stash}` saved")} |");
    sb.AppendLine($"| Now on        | `{r.NowOn}` |");
    return sb.ToString();
}

string RenderTaskUpdate(TaskUpdateResult r)
{
    var sb = new StringBuilder();
    var f = r.Wi.GetProperty("fields");
    string F(string n) => f.TryGetProperty(n, out var v) ? ExtractString(v) : "—";
    sb.AppendLine($"## Task Updated — #{r.WiId}");
    sb.AppendLine();
    sb.AppendLine("| Field | Value |");
    sb.AppendLine("|-------|-------|");
    sb.AppendLine($"| Title    | {F("System.Title")} |");
    sb.AppendLine($"| State    | {F("System.State")} |");
    sb.AppendLine($"| Assigned | {F("System.AssignedTo")} |");
    sb.AppendLine($"| Tags     | {F("System.Tags")} |");
    return sb.ToString();
}

string RenderTaskComplete(TaskCompleteResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine("## Activity Complete");
    sb.AppendLine();
    sb.AppendLine("| | |");
    sb.AppendLine("|-|---|");
    sb.AppendLine($"| WI     | #{r.WiId} — {r.WiState} |");
    sb.AppendLine($"| PR     | #{r.PrId} — {r.PrState} |");
    sb.AppendLine($"| Now on | `{r.NowOn}` (up to date) |");
    return sb.ToString();
}

string RenderTaskSync(TaskSyncResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine($"## Task Sync — `{r.Branch}`");
    sb.AppendLine();
    sb.AppendLine($"  Strategy   {r.Strategy}");
    sb.AppendLine($"  From       origin/{r.Target}  ({r.Behind} commits behind)");
    sb.AppendLine($"  Result     {(r.Behind == 0 ? "already up to date" : "clean merge")}  →  {(r.Pushed ? "pushed" : "not pushed")}");
    if (r.CommitsMerged.Count > 0)
    {
        sb.AppendLine();
        sb.AppendLine("  Commits merged:");
        foreach (var c in r.CommitsMerged) sb.AppendLine($"  - {c}");
    }
    return sb.ToString();
}

string RenderTaskSyncCheck(TaskSyncCheckResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine($"## Task Sync Check — `{r.Branch}`");
    sb.AppendLine();
    sb.AppendLine($"  Branch is {r.Behind} commits behind origin/{r.Target}, {r.Ahead} commits ahead.");
    if (r.BehindCommits.Count > 0)
    {
        sb.AppendLine();
        sb.AppendLine($"  Behind (not yet in branch):");
        foreach (var c in r.BehindCommits) sb.AppendLine($"  - {c}");
    }
    if (r.AheadCommits.Count > 0)
    {
        sb.AppendLine();
        sb.AppendLine($"  Ahead (not yet merged):");
        foreach (var c in r.AheadCommits) sb.AppendLine($"  - {c}");
    }
    sb.AppendLine();
    sb.AppendLine("  Run `fm task sync` to merge, or `fm task sync --rebase` to rebase.");
    return sb.ToString();
}

string RenderPrShow(PrShowResult r)
{
    var pr = r.Pr;
    string F(string n) => pr.TryGetProperty(n, out var v) ? ExtractString(v) : "—";
    var isDraft = pr.TryGetProperty("isDraft", out var d) && d.GetBoolean();
    var status = isDraft ? "draft" : F("status");
    var source = StripRefsHeads(F("sourceRefName"));
    var target = StripRefsHeads(F("targetRefName"));
    var createdBy = pr.TryGetProperty("createdBy", out var cb) && cb.TryGetProperty("displayName", out var dn) ? dn.GetString() ?? "—" : "—";
    var created = F("creationDate");
    if (DateTimeOffset.TryParse(created, out var dto)) created = dto.ToString("yyyy-MM-dd");
    var reviewers = pr.TryGetProperty("reviewers", out var rev) ? rev.GetArrayLength() : 0;
    var mergeStatus = F("mergeStatus");
    var wiList = new List<string>();
    if (pr.TryGetProperty("workItemRefs", out var wirefs))
    {
        foreach (var w in wirefs.EnumerateArray())
            wiList.Add($"#{w.GetProperty("id").GetString()}");
    }

    var sb = new StringBuilder();
    sb.AppendLine($"## PR #{r.PrId} — {F("title")}");
    sb.AppendLine();
    sb.AppendLine("| Field      | Value |");
    sb.AppendLine("|------------|-------|");
    sb.AppendLine($"| State      | {status} |");
    sb.AppendLine($"| Branches   | `{source}` → `{target}` |");
    sb.AppendLine($"| Created By | {createdBy} |");
    sb.AppendLine($"| Created    | {created} |");
    sb.AppendLine($"| Reviewers  | {reviewers} |");
    sb.AppendLine($"| Linked WI  | {(wiList.Count == 0 ? "—" : string.Join(", ", wiList))} |");
    sb.AppendLine($"| Mergeable  | {mergeStatus} |");
    return sb.ToString();
}

string RenderPrMerge(PrMergeResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine($"## PR Merged — #{r.PrId}");
    sb.AppendLine();
    sb.AppendLine("| | |");
    sb.AppendLine("|-|---|");
    sb.AppendLine($"| Strategy  | {r.Strategy} |");
    sb.AppendLine($"| PR        | #{r.PrId} — completed |");
    sb.AppendLine($"| WI        | #{r.WiId} — Closed |");
    sb.AppendLine($"| Merged to | `{r.MergedTo}` |");
    sb.AppendLine($"| Commit    | {r.Commit} |");
    sb.AppendLine();
    sb.AppendLine("  Run `fm task complete` to switch to baseline and pull.");
    return sb.ToString();
}

string RenderPrMergeError(PrMergeErrorResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine("## Error — PR Not Mergeable");
    sb.AppendLine();
    sb.AppendLine($"  PR #{r.PrId} cannot be merged.");
    sb.AppendLine();
    sb.AppendLine("  Failures:");
    foreach (var f in r.Failures) sb.AppendLine($"  - {f}");
    sb.AppendLine();
    sb.AppendLine("  Resolve the above, then re-run `fm pr merge`.");
    return sb.ToString();
}

string RenderPrReview(PrReviewResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine($"## PR Review — #{r.PrId}");
    sb.AppendLine();
    sb.AppendLine($"  Now on branch  `{r.Branch}`");
    if (r.OriginalBranch is not null)
        sb.AppendLine($"  Held activity  `{r.OriginalBranch}`{(r.StashName is null ? "" : $"  (stash: `{r.StashName}`)")}");
    sb.AppendLine();
    sb.AppendLine("  Resume with `fm work load <wi-id>`.");
    return sb.ToString();
}

string RenderTaskSyncConflict(TaskSyncConflictResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine("## Task Sync — CONFLICT");
    sb.AppendLine();
    sb.AppendLine($"  Strategy   {r.Strategy}");
    sb.AppendLine($"  From       origin/{r.Target}");
    sb.AppendLine();
    sb.AppendLine("  Conflicting files:");
    if (r.ConflictingFiles.Count == 0) sb.AppendLine("  (none reported by git status)");
    else foreach (var f in r.ConflictingFiles) sb.AppendLine($"  - {f}");
    sb.AppendLine();
    sb.AppendLine("  Resolve conflicts manually with git, then run `fm push`.");
    return sb.ToString();
}

// ── ADO REST: low-level fetch helpers ─────────────────────────────────────────

async Task<JsonElement?> GetWi(HttpClient http, int id, bool expandRelations = false)
{
    var url = expandRelations ? V($"wit/workitems/{id}?$expand=relations") : V($"wit/workitems/{id}");
    var resp = await http.GetAsync(url);
    if (resp.StatusCode == System.Net.HttpStatusCode.NotFound) return null;
    if (await ExitOnError(resp) != 0) return null;
    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    return doc.RootElement.Clone();
}

async Task<List<int>> WiqlIds(HttpClient http, string query, int top = 200)
{
    var body = new StringContent(JsonSerializer.Serialize(new { query }), Encoding.UTF8, "application/json");
    var resp = await http.PostAsync($"wit/wiql?$top={top}&api-version=7.1", body);
    if (await ExitOnError(resp) != 0) return new();
    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    var root = doc.RootElement;
    if (root.TryGetProperty("workItems", out var wis))
        return wis.EnumerateArray().Select(e => e.GetProperty("id").GetInt32()).ToList();
    if (root.TryGetProperty("workItemRelations", out var rels))
        return rels.EnumerateArray()
            .Where(r => r.TryGetProperty("target", out _))
            .Select(r => r.GetProperty("target").GetProperty("id").GetInt32())
            .Distinct()
            .ToList();
    return new();
}

async Task<List<JsonElement>> GetWisBatch(HttpClient http, IEnumerable<int> ids)
{
    var idList = ids.ToList();
    if (idList.Count == 0) return new();
    var all = new List<JsonElement>();
    foreach (var batch in idList.Chunk(200))
    {
        var csv = string.Join(",", batch);
        var resp = await http.GetAsync(V($"wit/workitems?ids={csv}&$expand=relations"));
        if (await ExitOnError(resp) != 0) return all;
        using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
        all.AddRange(doc.RootElement.GetProperty("value").EnumerateArray().Select(e => e.Clone()));
    }
    return all;
}

async Task<JsonElement?> PatchWi(HttpClient http, int id, params (string, object?)[] ops)
{
    var resp = await http.PatchAsync(V($"wit/workitems/{id}"), BuildPatch(ops));
    if (await ExitOnError(resp) != 0) return null;
    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    return doc.RootElement.Clone();
}

async Task<JsonElement?> CreateWi(HttpClient http, string type, params (string, object?)[] ops)
{
    var resp = await http.PostAsync(V($"wit/workitems/${Uri.EscapeDataString(type)}"), BuildPatch(ops));
    if (await ExitOnError(resp) != 0) return null;
    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    return doc.RootElement.Clone();
}

async Task<bool> AddWiRelation(HttpClient http, int wiId, string relType, string url, JsonObject? attrs = null)
{
    var rel = new JsonObject
    {
        ["rel"] = relType,
        ["url"] = url
    };
    if (attrs is not null) rel["attributes"] = attrs;
    var patch = new JsonArray(new JsonObject
    {
        ["op"]    = "add",
        ["path"]  = "/relations/-",
        ["value"] = rel
    });
    var content = new StringContent(patch.ToJsonString(), Encoding.UTF8, "application/json-patch+json");
    var resp = await http.PatchAsync(V($"wit/workitems/{wiId}"), content);
    if (await ExitOnError(resp) != 0) return false;
    return true;
}

async Task<JsonElement?> GetRepoInfo(HttpClient http, string repo)
{
    var resp = await http.GetAsync(V($"git/repositories/{Uri.EscapeDataString(repo)}"));
    if (await ExitOnError(resp) != 0) return null;
    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    return doc.RootElement.Clone();
}

async Task<string?> GetBranchSha(HttpClient http, string repo, string branchName)
{
    var resp = await http.GetAsync(V($"git/repositories/{Uri.EscapeDataString(repo)}/refs?filter=heads/{Uri.EscapeDataString(branchName)}"));
    if (await ExitOnError(resp) != 0) return null;
    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    var arr = doc.RootElement.GetProperty("value");
    if (arr.GetArrayLength() == 0) return null;
    return arr[0].GetProperty("objectId").GetString();
}

async Task<bool> CreateBranch(HttpClient http, string repo, string branchName, string baseSha)
{
    var body = new JsonArray(new JsonObject
    {
        ["name"]        = $"refs/heads/{branchName}",
        ["newObjectId"] = baseSha,
        ["oldObjectId"] = "0000000000000000000000000000000000000000"
    });
    var resp = await http.PostAsync(
        V($"git/repositories/{Uri.EscapeDataString(repo)}/refs"),
        new StringContent(body.ToJsonString(), Encoding.UTF8, "application/json"));
    if (await ExitOnError(resp) != 0) return false;
    return true;
}

async Task<List<JsonElement>> GetPrsBySource(HttpClient http, string repo, string branchName, string status = "active")
{
    var qs = $"git/repositories/{Uri.EscapeDataString(repo)}/pullrequests?searchCriteria.sourceRefName={Uri.EscapeDataString(NormalizeRef(branchName))}&searchCriteria.status={status}";
    var resp = await http.GetAsync(V(qs));
    if (await ExitOnError(resp) != 0) return new();
    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    return doc.RootElement.GetProperty("value").EnumerateArray().Select(e => e.Clone()).ToList();
}

async Task<JsonElement?> GetPr(HttpClient http, string repo, int prId)
{
    var resp = await http.GetAsync(V($"git/repositories/{Uri.EscapeDataString(repo)}/pullrequests/{prId}?$expand=all"));
    if (resp.StatusCode == System.Net.HttpStatusCode.NotFound) return null;
    if (await ExitOnError(resp) != 0) return null;
    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    return doc.RootElement.Clone();
}

async Task<JsonElement?> CreatePr(HttpClient http, string repo, string source, string target, string title, int? wiId, bool draft, string? description)
{
    var body = new JsonObject
    {
        ["sourceRefName"] = NormalizeRef(source),
        ["targetRefName"] = NormalizeRef(target),
        ["title"]         = title,
        ["isDraft"]       = draft
    };
    if (description is not null) body["description"] = description;
    if (wiId.HasValue)
        body["workItemRefs"] = new JsonArray(new JsonObject { ["id"] = wiId.Value.ToString() });
    var resp = await http.PostAsync(
        V($"git/repositories/{Uri.EscapeDataString(repo)}/pullrequests"),
        new StringContent(body.ToJsonString(), Encoding.UTF8, "application/json"));
    if (await ExitOnError(resp) != 0) return null;
    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    return doc.RootElement.Clone();
}

async Task<JsonElement?> PatchPr(HttpClient http, string repo, int prId, JsonObject body)
{
    var req = new HttpRequestMessage(HttpMethod.Patch,
        V($"git/repositories/{Uri.EscapeDataString(repo)}/pullrequests/{prId}"))
    {
        Content = new StringContent(body.ToJsonString(), Encoding.UTF8, "application/json")
    };
    var resp = await http.SendAsync(req);
    if (await ExitOnError(resp) != 0) return null;
    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    return doc.RootElement.Clone();
}

// ── idempotent ensure-* helpers ───────────────────────────────────────────────

async Task<int> EnsureWi(HttpClient http, string title, string type, string? description, string? tags, string? assignedTo)
{
    var project = Environment.GetEnvironmentVariable("ADO_PROJECT")!;
    var escTitle = title.Replace("'", "''");
    var query = $"SELECT [System.Id] FROM WorkItems WHERE [System.TeamProject] = '{project}' AND [System.Title] = '{escTitle}' AND [System.WorkItemType] = '{type}'";
    var ids = await WiqlIds(http, query, 5);
    if (ids.Count > 0)
    {
        Console.Error.WriteLine($"  WI #{ids[0]} matched by title — reusing.");
        return ids[0];
    }
    var created = await CreateWi(http, type,
        ("/fields/System.Title",       title),
        ("/fields/System.Description", description),
        ("/fields/System.AssignedTo",  assignedTo),
        ("/fields/System.Tags",        tags))
        ?? throw new InvalidOperationException("WI creation failed.");
    var newId = created.GetProperty("id").GetInt32();
    Console.Error.WriteLine($"  WI #{newId} created.");
    return newId;
}

async Task<bool> EnsureBranch(HttpClient http, string repo, string branchName, string targetBranch)
{
    var existing = await GetBranchSha(http, repo, branchName);
    if (existing is not null)
    {
        Console.Error.WriteLine($"  Branch '{branchName}' already exists — reusing.");
        return true;
    }
    var baseSha = await GetBranchSha(http, repo, targetBranch)
        ?? throw new InvalidOperationException($"Target branch '{targetBranch}' not found.");
    var ok = await CreateBranch(http, repo, branchName, baseSha);
    if (ok) Console.Error.WriteLine($"  Branch '{branchName}' created from '{targetBranch}'.");
    return ok;
}

async Task<(int prId, JsonElement pr)> EnsurePr(HttpClient http, string repo, string source, string target, string title, int wiId, bool draft, string? description)
{
    foreach (var status in new[] { "active" })
    {
        var existing = await GetPrsBySource(http, repo, source, status);
        if (existing.Count > 1)
        {
            Console.Error.WriteLine($"Error: multiple {status} PRs for branch '{source}'.");
            foreach (var p in existing)
                Console.Error.WriteLine($"  #{p.GetProperty("pullRequestId").GetInt32()} — {p.GetProperty("title").GetString()}");
            Environment.Exit(1);
        }
        if (existing.Count == 1)
        {
            var pr = existing[0];
            var prId = pr.GetProperty("pullRequestId").GetInt32();
            Console.Error.WriteLine($"  PR #{prId} already exists for branch — reusing.");
            return (prId, pr);
        }
    }
    var created = await CreatePr(http, repo, source, target, title, wiId, draft, description)
        ?? throw new InvalidOperationException("PR creation failed.");
    var newId = created.GetProperty("pullRequestId").GetInt32();
    Console.Error.WriteLine($"  PR #{newId} created (draft={draft}).");
    return (newId, created);
}

async Task<bool> EnsureWiState(HttpClient http, int wiId, string state)
{
    var wi = await GetWi(http, wiId) ?? throw new InvalidOperationException($"WI #{wiId} not found.");
    var current = wi.GetProperty("fields").TryGetProperty("System.State", out var s) ? s.GetString() : null;
    if (string.Equals(current, state, StringComparison.OrdinalIgnoreCase))
    {
        Console.Error.WriteLine($"  WI #{wiId} already in state '{state}'.");
        return true;
    }
    var updated = await PatchWi(http, wiId, ("/fields/System.State", state));
    if (updated is not null) Console.Error.WriteLine($"  WI #{wiId} state: '{current}' → '{state}'.");
    return updated is not null;
}

async Task<bool> EnsureWiLink(HttpClient http, int wiId, string relType, string artifactUri)
{
    var wi = await GetWi(http, wiId, expandRelations: true);
    if (wi is null) return false;
    if (wi.Value.TryGetProperty("relations", out var rels))
    {
        foreach (var r in rels.EnumerateArray())
        {
            var rUrl = r.TryGetProperty("url", out var u) ? u.GetString() : null;
            if (string.Equals(rUrl, artifactUri, StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }
        }
    }
    var ok = await AddWiRelation(http, wiId, relType, artifactUri);
    if (ok) Console.Error.WriteLine($"  Linked WI #{wiId} → {relType}");
    return ok;
}

string BranchArtifactUri(string projectId, string repoId, string branchName)
    => $"vstfs:///Git/Ref/{projectId}%2F{repoId}%2FGB{Uri.EscapeDataString(branchName)}";

string PrArtifactUri(string projectId, string repoId, int prId)
    => $"vstfs:///Git/PullRequestId/{projectId}%2F{repoId}%2F{prId}";

async Task<bool> ValidateActivityInvariants(HttpClient http, string repo, BranchContext ctx)
{
    if (!ctx.IsActivity) return true;
    var wiId = ctx.WorkItemId!.Value;

    var wi = await GetWi(http, wiId, expandRelations: true);
    if (wi is null)
    {
        Console.Error.WriteLine($"Error: WI #{wiId} not found (referenced by branch).");
        Environment.Exit(1);
        return false;
    }

    var sha = await GetBranchSha(http, repo, ctx.BranchName);
    if (sha is null)
    {
        Console.Error.WriteLine($"Error: remote branch '{ctx.BranchName}' missing — run `fm work load {wiId}` to repair.");
        Environment.Exit(1);
        return false;
    }

    var prs = await GetPrsBySource(http, repo, ctx.BranchName, "active");
    if (prs.Count == 0)
    {
        Console.Error.WriteLine($"Error: no active PR for branch '{ctx.BranchName}' — run `fm work load {wiId}` to repair.");
        Environment.Exit(1);
        return false;
    }
    var prId = prs[0].GetProperty("pullRequestId").GetInt32();

    var repoInfo = await GetRepoInfo(http, repo);
    if (repoInfo is not null)
    {
        var repoId    = repoInfo.Value.GetProperty("id").GetString()!;
        var projectId = repoInfo.Value.GetProperty("project").GetProperty("id").GetString()!;
        await EnsureWiLink(http, wiId, "ArtifactLink", BranchArtifactUri(projectId, repoId, ctx.BranchName));
        await EnsureWiLink(http, wiId, "ArtifactLink", PrArtifactUri(projectId, repoId, prId));
    }
    return true;
}

// ── ID disambiguation ─────────────────────────────────────────────────────────

async Task<ResolvedId> ResolveId(HttpClient http, string repo, string raw)
{
    raw = raw.Trim();

    var branchMatch = Regex.Match(raw, @"^(feature|fix)/(\d+)-");
    if (branchMatch.Success)
    {
        var wiId = int.Parse(branchMatch.Groups[2].Value);
        return new ResolvedId(WiId: wiId, PrId: null, Source: "branch");
    }

    if (raw.StartsWith("w-") || raw.StartsWith("wi-") || raw.StartsWith("w") && raw.Length > 1 && char.IsDigit(raw[1]))
    {
        var num = new string(raw.Where(char.IsDigit).ToArray());
        if (int.TryParse(num, out var wid)) return new ResolvedId(wid, null, "wi-prefix");
    }

    if (raw.StartsWith("pr-") || raw.StartsWith("p-"))
    {
        var num = new string(raw.Where(char.IsDigit).ToArray());
        if (int.TryParse(num, out var pid)) return new ResolvedId(null, pid, "pr-prefix");
    }

    if (int.TryParse(raw, out var n))
    {
        var wi = await GetWi(http, n);
        var pr = await GetPr(http, repo, n);
        if (wi is not null && pr is not null)
        {
            Console.Error.WriteLine($"Error: id '{raw}' matches both WI and PR — disambiguate with `w-{n}` or `pr-{n}`.");
            Environment.Exit(1);
        }
        if (wi is not null) return new ResolvedId(n, null, "ambiguous-wi");
        if (pr is not null) return new ResolvedId(null, n, "ambiguous-pr");
        Console.Error.WriteLine($"Error: id '{raw}' is neither a known WI nor PR.");
        Environment.Exit(1);
    }

    Console.Error.WriteLine($"Error: cannot parse id '{raw}'.");
    Environment.Exit(1);
    return new ResolvedId(null, null, "error");
}

// ── git wrapper ───────────────────────────────────────────────────────────────

static class Git
{
    public static (int exit, string stdout, string stderr) Run(params string[] args)
        => RunIn(null, args);

    public static (int exit, string stdout, string stderr) RunIn(string? cwd, params string[] args)
    {
        var psi = new ProcessStartInfo("git")
        {
            RedirectStandardOutput = true,
            RedirectStandardError  = true,
            UseShellExecute        = false,
            CreateNoWindow         = true,
        };
        if (cwd is not null) psi.WorkingDirectory = cwd;
        foreach (var a in args) psi.ArgumentList.Add(a);

        using var proc = Process.Start(psi)!;
        var stdout = proc.StandardOutput.ReadToEnd();
        var stderr = proc.StandardError.ReadToEnd();
        proc.WaitForExit();
        return (proc.ExitCode, stdout, stderr);
    }

    public static (int exit, string stdout, string stderr) RunPassthrough(params string[] args)
    {
        var psi = new ProcessStartInfo("git")
        {
            RedirectStandardOutput = false,
            RedirectStandardError  = false,
            UseShellExecute        = false,
        };
        foreach (var a in args) psi.ArgumentList.Add(a);
        using var proc = Process.Start(psi)!;
        proc.WaitForExit();
        return (proc.ExitCode, "", "");
    }

    public static string CurrentBranch()
    {
        var (_, s, _) = Run("rev-parse", "--abbrev-ref", "HEAD");
        return s.Trim();
    }

    public static bool IsClean()
    {
        var (_, s, _) = Run("status", "--porcelain");
        return string.IsNullOrWhiteSpace(s);
    }

    public static (int ahead, int behind) AheadBehind(string upstream)
    {
        var (exit, s, _) = Run("rev-list", "--left-right", "--count", $"{upstream}...HEAD");
        if (exit != 0) return (0, 0);
        var parts = s.Trim().Split('\t');
        if (parts.Length != 2) return (0, 0);
        return (int.TryParse(parts[1], out var a) ? a : 0,
                int.TryParse(parts[0], out var b) ? b : 0);
    }
}

// ── records ───────────────────────────────────────────────────────────────────

record BranchContext(bool IsActivity, string BranchName, string? ActivityType, int? WorkItemId, string? Slug);

record ErrorResult(string Title, string Message);

record ResolvedId(int? WiId, int? PrId, string Source);

record WorkActivityResult(
    int Id, string Title, string Type, string State,
    string Branch, int PrId, string PrState, string Target,
    string Mergeable, string? StashRestored, string Action);

record WorkClosedResult(int Id, string Title, string State, int? MergedPrId);

record WorkListResult(int Count, List<JsonElement> Items);

record ContextBaselineResult(string Branch, List<string> RecentCommits);
record ContextActivityResult(
    string Branch, JsonElement? Wi, int? PrId, JsonElement? Pr,
    int Ahead, int Behind, bool Clean,
    string? PipelineName, int? PipelineId, JsonElement? LatestRun,
    List<JsonElement> Todos,
    bool OnlyWi, bool OnlyPr, bool OnlyGit, bool OnlyPipeline,
    string Target);

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

// PHASE_RECORDS_MARKER — phases 3-9 add their result records here.
