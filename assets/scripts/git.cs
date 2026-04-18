#!/usr/bin/dotnet run
#:package System.CommandLine@2.0.0-beta4.22272.1
#:package YamlDotNet@16.3.0
#:property PublishAot=false
#:property PublishTrimmed=false

#nullable enable

using System.CommandLine;
using System.Diagnostics;
using System.Text.Json;
using System.Text.RegularExpressions;
using YamlDotNet.Serialization;
using YamlDotNet.Serialization.NamingConventions;

var jsonOpts = new JsonSerializerOptions { WriteIndented = true };
var yaml = new SerializerBuilder()
    .WithNamingConvention(CamelCaseNamingConvention.Instance)
    .Build();

var formatOpt = new Option<string>("--format", () => "json", "Output format: json, yaml, markdown");

var branchCurrentCmd = new Command("branch-current", "Get current branch name") { formatOpt };
branchCurrentCmd.SetHandler((string format) =>
{
    var result = RunGit("rev-parse", "--abbrev-ref", "HEAD");
    EnsureSuccess(result, "Cannot read current branch.");
    Render(format, new BranchResult(result.StdOut));
}, formatOpt);

var branchExistsNameOpt = new Option<string>("--name", "Branch name") { IsRequired = true };
var branchExistsRemoteOpt = new Option<bool>("--remote", "Check origin instead of local refs");
var branchExistsCmd = new Command("branch-exists", "Check if a branch exists")
{
    branchExistsNameOpt, branchExistsRemoteOpt, formatOpt
};
branchExistsCmd.SetHandler((string name, bool remote, string format) =>
{
    GitCommandResult result;
    bool exists;

    if (remote)
    {
        result = RunGit("ls-remote", "--heads", "origin", name);
        exists = result.ExitCode == 0 && !string.IsNullOrWhiteSpace(result.StdOut);
    }
    else
    {
        result = RunGit("show-ref", "--verify", $"refs/heads/{name}");
        exists = result.ExitCode == 0;
    }

    Render(format, new BranchExistsResult(name, remote, exists));
    if (!exists)
        Environment.Exit(1);
}, branchExistsNameOpt, branchExistsRemoteOpt, formatOpt);

var checkoutNameOpt = new Option<string>("--name", "Branch name") { IsRequired = true };
var checkoutCmd = new Command("checkout", "Checkout an existing local branch")
{
    checkoutNameOpt, formatOpt
};
checkoutCmd.SetHandler((string name, string format) =>
{
    var result = RunGit("checkout", name);
    EnsureSuccess(result, $"Cannot checkout '{name}'.");
    Render(format, new CheckoutResult(name, "local", result.StdOut));
}, checkoutNameOpt, formatOpt);

var checkoutRemoteCmd = new Command("checkout-remote", "Checkout a branch from origin, creating a local tracking branch if needed")
{
    checkoutNameOpt, formatOpt
};
checkoutRemoteCmd.SetHandler((string name, string format) =>
{
    if (LocalBranchExists(name))
    {
        var localCheckout = RunGit("checkout", name);
        EnsureSuccess(localCheckout, $"Cannot checkout '{name}'.");
        Render(format, new CheckoutResult(name, "local", localCheckout.StdOut));
        return;
    }

    var fetch = RunGit("fetch", "origin", name);
    EnsureSuccess(fetch, $"Cannot fetch origin/{name}.");

    var track = RunGit("checkout", "-b", name, "--track", $"origin/{name}");
    EnsureSuccess(track, $"Cannot checkout tracking branch '{name}'.");
    Render(format, new CheckoutResult(name, "remote", track.StdOut));
}, checkoutNameOpt, formatOpt);

var fetchAllOpt = new Option<bool>("--all", "Fetch all remotes");
var fetchCmd = new Command("fetch", "Fetch from remote")
{
    fetchAllOpt, formatOpt
};
fetchCmd.SetHandler((bool all, string format) =>
{
    var result = all ? RunGit("fetch", "--all") : RunGit("fetch", "origin");
    EnsureSuccess(result, "Fetch failed.");
    Render(format, new FetchResult(true, all, result.StdOut));
}, fetchAllOpt, formatOpt);

var pushForceOpt = new Option<bool>("--force", "Use --force-with-lease");
var pushSetUpstreamOpt = new Option<bool>("--set-upstream", "Set upstream to origin/<current-branch>");
var pushCmd = new Command("push", "Push current branch")
{
    pushForceOpt, pushSetUpstreamOpt, formatOpt
};
pushCmd.SetHandler((bool force, bool setUpstream, string format) =>
{
    var args = new List<string> { "push" };
    if (force)
        args.Add("--force-with-lease");
    if (setUpstream)
    {
        args.Add("--set-upstream");
        args.Add("origin");
        args.Add(CurrentBranch());
    }
    else
    {
        args.Add("origin");
        args.Add("HEAD");
    }

    var result = RunGit(args.ToArray());
    EnsureSuccess(result, "Push failed.");
    Render(format, new PushResult(true, force, setUpstream, result.StdOut));
}, pushForceOpt, pushSetUpstreamOpt, formatOpt);

var pullRebaseOpt = new Option<bool>("--rebase", "Use rebase instead of merge");
var pullCmd = new Command("pull", "Pull current branch")
{
    pullRebaseOpt, formatOpt
};
pullCmd.SetHandler((bool rebase, string format) =>
{
    var result = rebase ? RunGit("pull", "--rebase") : RunGit("pull");
    EnsureSuccess(result, "Pull failed.");
    Render(format, new PullResult(true, rebase, result.StdOut));
}, pullRebaseOpt, formatOpt);

var statusPorcelainOpt = new Option<bool>("--porcelain", "Use porcelain output");
var statusCmd = new Command("status", "Get working tree status")
{
    statusPorcelainOpt, formatOpt
};
statusCmd.SetHandler((bool porcelain, string format) =>
{
    var result = porcelain ? RunGit("status", "--porcelain") : RunGit("status", "--short");
    EnsureSuccess(result, "Status failed.");
    var entries = SplitLines(result.StdOut);
    Render(format, new StatusResult(entries.Count == 0, result.StdOut, entries));
}, statusPorcelainOpt, formatOpt);

var stashMessageOpt = new Option<string>("--message", "Stash message") { IsRequired = true };
var stashIncludeUntrackedOpt = new Option<bool>("--include-untracked", () => true, "Include untracked files");
var stashSaveCmd = new Command("stash-save", "Save a named stash if it does not already exist")
{
    stashMessageOpt, stashIncludeUntrackedOpt, formatOpt
};
stashSaveCmd.SetHandler((string message, bool includeUntracked, string format) =>
{
    var existing = FindStash(message);
    if (existing is not null)
    {
        Render(format, new StashSaveResult(existing.Ref, message, true));
        return;
    }

    var args = new List<string> { "stash", "push", "-m", message };
    if (includeUntracked)
        args.Insert(2, "-u");

    var result = RunGit(args.ToArray());
    EnsureSuccess(result, "Stash save failed.");

    var created = FindStash(message);
    Render(format, new StashSaveResult(created?.Ref ?? "stash@{0}", message, false));
}, stashMessageOpt, stashIncludeUntrackedOpt, formatOpt);

var stashPopNameOpt = new Option<string>("--name", "Substring to match in stash message") { IsRequired = true };
var stashPopCmd = new Command("stash-pop", "Pop a stash by matching its message")
{
    stashPopNameOpt, formatOpt
};
stashPopCmd.SetHandler((string name, string format) =>
{
    var stash = FindStash(name);
    if (stash is null)
    {
        Console.Error.WriteLine($"Stash '{name}' not found.");
        Environment.Exit(1);
        return;
    }

    var result = RunGit("stash", "pop", stash.Ref);
    EnsureSuccess(result, $"Stash pop failed for '{stash.Ref}'.");
    Render(format, new StashPopResult(stash.Ref, true));
}, stashPopNameOpt, formatOpt);

var stashListFilterOpt = new Option<string?>("--filter", () => null, "Case-insensitive substring match");
var stashListCmd = new Command("stash-list", "List stashes")
{
    stashListFilterOpt, formatOpt
};
stashListCmd.SetHandler((string? filter, string format) =>
{
    var result = RunGit("stash", "list");
    EnsureSuccess(result, "Stash list failed.");

    var stashes = ParseStashes(result.StdOut);
    if (!string.IsNullOrWhiteSpace(filter))
    {
        stashes = stashes
            .Where(x => x.Message.Contains(filter, StringComparison.OrdinalIgnoreCase))
            .ToList();
    }

    Render(format, new StashListResult(stashes.Count, stashes));
}, stashListFilterOpt, formatOpt);

var logOnelineOpt = new Option<bool>("--oneline", "Use one-line format");
var logMaxCountOpt = new Option<int>("--max-count", () => 10, "Maximum number of commits");
var logBranchOpt = new Option<string?>("--branch", () => null, "Branch/range to inspect");
var logCmd = new Command("log", "Get commit log")
{
    logOnelineOpt, logMaxCountOpt, logBranchOpt, formatOpt
};
logCmd.SetHandler((bool oneline, int maxCount, string? branch, string format) =>
{
    var target = string.IsNullOrWhiteSpace(branch) ? "HEAD" : branch;
    var args = new List<string> { "log", target, $"--max-count={Math.Max(1, maxCount)}" };
    if (oneline)
        args.Add("--oneline");

    var result = RunGit(args.ToArray());
    EnsureSuccess(result, "Log failed.");
    var commits = SplitLines(result.StdOut);
    Render(format, new LogResult(commits.Count, commits));
}, logOnelineOpt, logMaxCountOpt, logBranchOpt, formatOpt);

var diffTargetOpt = new Option<string>("--target", "Target ref") { IsRequired = true };
var diffCmd = new Command("diff", "Get ahead/behind information relative to a target")
{
    diffTargetOpt, formatOpt
};
diffCmd.SetHandler((string target, string format) =>
{
    var countResult = RunGit("rev-list", "--left-right", "--count", $"HEAD...{target}");
    EnsureSuccess(countResult, $"Cannot diff against '{target}'.");

    var parts = countResult.StdOut.Split('\t', StringSplitOptions.RemoveEmptyEntries);
    if (parts.Length != 2 ||
        !int.TryParse(parts[0], out var ahead) ||
        !int.TryParse(parts[1], out var behind))
    {
        Console.Error.WriteLine($"Unexpected diff output: {countResult.StdOut}");
        Environment.Exit(1);
        return;
    }

    var aheadLog = RunGit("log", "--oneline", $"{target}..HEAD");
    var behindLog = RunGit("log", "--oneline", $"HEAD..{target}");
    EnsureSuccess(aheadLog, "Cannot inspect ahead commits.");
    EnsureSuccess(behindLog, "Cannot inspect behind commits.");

    Render(format, new DiffResult(
        target,
        ahead,
        behind,
        SplitLines(aheadLog.StdOut),
        SplitLines(behindLog.StdOut)));
}, diffTargetOpt, formatOpt);

var mergeTargetOpt = new Option<string>("--target", "Target ref to merge") { IsRequired = true };
var mergeCmd = new Command("merge", "Merge a target ref into the current branch")
{
    mergeTargetOpt, formatOpt
};
mergeCmd.SetHandler((string target, string format) =>
{
    var result = RunGit("merge", target);
    var conflicts = GetConflictFiles();
    Render(format, new IntegrationResult("merge", target, result.ExitCode == 0, conflicts, result.StdOut, result.StdErr));
    if (result.ExitCode != 0)
        Environment.Exit(1);
}, mergeTargetOpt, formatOpt);

var rebaseTargetOpt = new Option<string>("--target", "Target ref to rebase onto") { IsRequired = true };
var rebaseCmd = new Command("rebase", "Rebase the current branch onto a target ref")
{
    rebaseTargetOpt, formatOpt
};
rebaseCmd.SetHandler((string target, string format) =>
{
    var result = RunGit("rebase", target);
    var conflicts = GetConflictFiles();
    Render(format, new IntegrationResult("rebase", target, result.ExitCode == 0, conflicts, result.StdOut, result.StdErr));
    if (result.ExitCode != 0)
        Environment.Exit(1);
}, rebaseTargetOpt, formatOpt);

var discardCmd = new Command("discard", "Discard local modifications in the current worktree") { formatOpt };
discardCmd.SetHandler((string format) =>
{
    var restore = RunGit("restore", "--source=HEAD", "--staged", "--worktree", ".");
    EnsureSuccess(restore, "Discard failed.");
    var clean = RunGit("clean", "-fd");
    EnsureSuccess(clean, "Clean failed.");
    Render(format, new DiscardResult(true));
}, formatOpt);

var remoteNameOpt = new Option<string>("--name", () => "origin", "Remote name");
var remoteGetUrlCmd = new Command("remote-get-url", "Get a remote URL")
{
    remoteNameOpt, formatOpt
};
remoteGetUrlCmd.SetHandler((string name, string format) =>
{
    var result = RunGit("remote", "get-url", name);
    EnsureSuccess(result, $"Cannot get URL for remote '{name}'.");
    Render(format, new RemoteUrlResult(name, result.StdOut));
}, remoteNameOpt, formatOpt);

var commitMessageOpt = new Option<string?>("--message", () => null, "Commit message");
var commitAllOpt = new Option<bool>("--all", "Stage tracked changes before commit");
var commitAmendOpt = new Option<bool>("--amend", "Amend the previous commit");
var commitCmd = new Command("commit", "Commit changes in the current repository")
{
    commitMessageOpt, commitAllOpt, commitAmendOpt, formatOpt
};
commitCmd.SetHandler((string? message, bool all, bool amend, string format) =>
{
    var args = new List<string> { "commit" };
    if (all)
        args.Add("--all");
    if (amend)
        args.Add("--amend");
    if (!string.IsNullOrWhiteSpace(message))
    {
        args.Add("-m");
        args.Add(message);
    }

    var result = RunGit(args.ToArray());
    EnsureSuccess(result, "Commit failed.");
    Render(format, BuildCommitResult(null, amend));
}, commitMessageOpt, commitAllOpt, commitAmendOpt, formatOpt);

var headCmd = new Command("head", "Get the current HEAD commit") { formatOpt };
headCmd.SetHandler((string format) =>
{
    Render(format, ReadHeadCommit(null));
}, formatOpt);

var stagePathOpt = new Option<string>("--path", "Path to stage") { IsRequired = true };
var stageCmd = new Command("stage", "Stage a path")
{
    stagePathOpt, formatOpt
};
stageCmd.SetHandler((string path, string format) =>
{
    var result = RunGit("add", path);
    EnsureSuccess(result, $"Cannot stage '{path}'.");
    Render(format, new StageResult(path, true));
}, stagePathOpt, formatOpt);

var stageAllPathOpt = new Option<string?>("--path", () => null, "Optional subdirectory to stage");
var stageAllCmd = new Command("stage-all", "Stage all changes")
{
    stageAllPathOpt, formatOpt
};
stageAllCmd.SetHandler((string? path, string format) =>
{
    GitCommandResult result;
    if (string.IsNullOrWhiteSpace(path))
        result = RunGit("add", "-A");
    else
        result = RunGit("add", "-A", path);

    EnsureSuccess(result, "Cannot stage changes.");
    Render(format, new StageResult(path ?? ".", true));
}, stageAllPathOpt, formatOpt);

var stageAllInPathOpt = new Option<string>("--path", "Working directory") { IsRequired = true };
var stageAllInCmd = new Command("stage-all-in", "Stage all changes inside another repository path")
{
    stageAllInPathOpt, formatOpt
};
stageAllInCmd.SetHandler((string path, string format) =>
{
    var result = RunGitIn(path, "add", "-A");
    EnsureSuccess(result, $"Cannot stage changes in '{path}'.");
    Render(format, new StageResult(path, true));
}, stageAllInPathOpt, formatOpt);

var submodulePathOpt = new Option<string>("--path", () => "_docs", "Submodule path");
var submoduleInspectCmd = new Command("submodule-inspect", "Inspect a git submodule/worktree")
{
    submodulePathOpt, formatOpt
};
submoduleInspectCmd.SetHandler((string path, string format) =>
{
    var inspection = InspectRepository(path);
    Render(format, inspection);
    if (!inspection.Exists)
        Environment.Exit(1);
}, submodulePathOpt, formatOpt);

var commitInPathOpt = new Option<string>("--path", "Working directory") { IsRequired = true };
var commitInCmd = new Command("commit-in", "Commit changes in another repository path")
{
    commitInPathOpt, commitMessageOpt, commitAllOpt, commitAmendOpt, formatOpt
};
commitInCmd.SetHandler((string path, string? message, bool all, bool amend, string format) =>
{
    var args = new List<string> { "commit" };
    if (all)
        args.Add("--all");
    if (amend)
        args.Add("--amend");
    if (!string.IsNullOrWhiteSpace(message))
    {
        args.Add("-m");
        args.Add(message);
    }

    var result = RunGitIn(path, args.ToArray());
    EnsureSuccess(result, $"Commit failed in '{path}'.");
    Render(format, BuildCommitResult(path, amend));
}, commitInPathOpt, commitMessageOpt, commitAllOpt, commitAmendOpt, formatOpt);

var pushInPathOpt = new Option<string>("--path", "Working directory") { IsRequired = true };
var pushInCmd = new Command("push-in", "Push the repository at another path")
{
    pushInPathOpt, pushForceOpt, pushSetUpstreamOpt, formatOpt
};
pushInCmd.SetHandler((string path, bool force, bool setUpstream, string format) =>
{
    var branch = CurrentBranch(path);
    var args = new List<string> { "push" };
    if (force)
        args.Add("--force-with-lease");
    if (setUpstream)
    {
        args.Add("--set-upstream");
        args.Add("origin");
        args.Add(branch);
    }
    else
    {
        args.Add("origin");
        args.Add("HEAD");
    }

    var result = RunGitIn(path, args.ToArray());
    EnsureSuccess(result, $"Push failed in '{path}'.");
    Render(format, new PushResult(true, force, setUpstream, result.StdOut));
}, pushInPathOpt, pushForceOpt, pushSetUpstreamOpt, formatOpt);

var rootCmd = new RootCommand("Structured git plumbing for fm workflow")
{
    branchCurrentCmd,
    branchExistsCmd,
    checkoutCmd,
    checkoutRemoteCmd,
    fetchCmd,
    pushCmd,
    pullCmd,
    statusCmd,
    stashSaveCmd,
    stashPopCmd,
    stashListCmd,
    logCmd,
    diffCmd,
    mergeCmd,
    rebaseCmd,
    discardCmd,
    remoteGetUrlCmd,
    commitCmd,
    headCmd,
    stageCmd,
    stageAllCmd,
    stageAllInCmd,
    submoduleInspectCmd,
    commitInCmd,
    pushInCmd
};

return await rootCmd.InvokeAsync(args);

static void EnsureSuccess(GitCommandResult result, string message)
{
    if (result.ExitCode == 0)
        return;

    if (!string.IsNullOrWhiteSpace(result.StdErr))
        Console.Error.WriteLine(result.StdErr.Trim());
    else if (!string.IsNullOrWhiteSpace(result.StdOut))
        Console.Error.WriteLine(result.StdOut.Trim());
    else
        Console.Error.WriteLine(message);

    Environment.Exit(1);
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
    BranchResult x => $"## Branch\n\n`{x.Name}`\n",
    BranchExistsResult x => $"## Branch Exists\n\n`{x.Name}`: {(x.Exists ? "yes" : "no")}\n",
    CheckoutResult x => $"## Checkout\n\n`{x.Branch}` ({x.Source})\n",
    StatusResult x => x.Clean ? "## Status\n\nclean\n" : $"## Status\n\n```\n{x.Status.TrimEnd()}\n```\n",
    StashSaveResult x => $"## Stash Saved\n\n`{x.StashRef}` {(x.AlreadyExisted ? "(already existed)" : "")}\n",
    StashPopResult x => $"## Stash Restored\n\n`{x.StashRef}`\n",
    LogResult x => x.Count == 0 ? "## Log\n\n_No commits._\n" : $"## Log\n\n{string.Join("\n", x.Commits.Select(c => $"- {c}"))}\n",
    DiffResult x => $"## Diff\n\nAhead: {x.Ahead}\nBehind: {x.Behind}\n",
    IntegrationResult x => x.Success
        ? $"## {Capitalize(x.Kind)}\n\nSuccess.\n"
        : $"## {Capitalize(x.Kind)}\n\nFailed.\n\n{RenderConflictMarkdown(x.Conflicts)}",
    RemoteUrlResult x => $"## Remote URL\n\n{x.Name}: `{x.Url}`\n",
    CommitResult x => $"## Commit\n\n`{x.ShortCommit}` {x.Subject}\n",
    RepositoryInspectionResult x => RenderInspectionMarkdown(x),
    _ => JsonSerializer.Serialize(data, jsonOpts)
};

string RenderConflictMarkdown(List<string> conflicts)
{
    if (conflicts.Count == 0)
        return "_No conflicting files reported._\n";

    return string.Join("\n", conflicts.Select(x => $"- {x}")) + "\n";
}

string RenderInspectionMarkdown(RepositoryInspectionResult x)
{
    if (!x.Exists)
        return $"## Repository\n\n`{x.Path}` not found.\n";

    var lines = new List<string>
    {
        "## Repository",
        "",
        $"Path: `{x.Path}`",
        $"Branch: `{x.Branch}`",
        $"Head: `{x.ShortCommit}` {x.Subject}",
        $"Clean: {(x.Clean ? "yes" : "no")}",
        $"Ahead: {x.Ahead}",
        $"Behind: {x.Behind}"
    };

    if (x.StatusEntries.Count > 0)
    {
        lines.Add("");
        lines.Add("Status:");
        lines.AddRange(x.StatusEntries.Select(s => $"- {s}"));
    }

    return string.Join('\n', lines) + "\n";
}

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
    IEnumerable<object> seq => seq.Select(ToYamlObject).ToList(),
    _ => value
};

static string Capitalize(string value)
    => string.IsNullOrWhiteSpace(value) ? value : char.ToUpperInvariant(value[0]) + value[1..];

static GitCommandResult RunGit(params string[] args) => RunGitIn(null, args);

static GitCommandResult RunGitIn(string? workingDirectory, params string[] args)
{
    var psi = new ProcessStartInfo("/usr/bin/git")
    {
        RedirectStandardOutput = true,
        RedirectStandardError = true,
        UseShellExecute = false
    };

    if (!string.IsNullOrWhiteSpace(workingDirectory))
        psi.WorkingDirectory = Path.GetFullPath(workingDirectory);

    psi.Environment["GIT_TERMINAL_PROMPT"] = "0";

    foreach (var arg in args)
        psi.ArgumentList.Add(arg);

    using var process = Process.Start(psi) ?? throw new InvalidOperationException("Failed to start git.");
    var stdout = process.StandardOutput.ReadToEnd().Trim();
    var stderr = process.StandardError.ReadToEnd().Trim();
    process.WaitForExit();
    return new GitCommandResult(process.ExitCode, stdout, stderr);
}

static string CurrentBranch(string? workingDirectory = null)
{
    var result = RunGitIn(workingDirectory, "rev-parse", "--abbrev-ref", "HEAD");
    EnsureSuccess(result, "Cannot read current branch.");
    return result.StdOut;
}

static bool LocalBranchExists(string name)
    => RunGit("show-ref", "--verify", $"refs/heads/{name}").ExitCode == 0;

static List<string> SplitLines(string value)
    => value.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries).ToList();

static List<string> GetConflictFiles()
{
    var result = RunGit("diff", "--name-only", "--diff-filter=U");
    if (result.ExitCode != 0 || string.IsNullOrWhiteSpace(result.StdOut))
        return new List<string>();
    return SplitLines(result.StdOut);
}

static List<StashInfo> ParseStashes(string stdout)
{
    var stashes = new List<StashInfo>();
    foreach (var line in SplitLines(stdout))
    {
        var match = Regex.Match(line, @"^(stash@\{\d+\}):\s+(.+)$");
        if (!match.Success)
            continue;

        stashes.Add(new StashInfo(match.Groups[1].Value, match.Groups[2].Value));
    }

    return stashes;
}

static StashInfo? FindStash(string message)
{
    var list = RunGit("stash", "list");
    if (list.ExitCode != 0)
        return null;

    return ParseStashes(list.StdOut)
        .FirstOrDefault(x => x.Message.Contains(message, StringComparison.OrdinalIgnoreCase));
}

static RepositoryInspectionResult InspectRepository(string path)
{
    var fullPath = Path.GetFullPath(path);
    if (!Directory.Exists(fullPath))
        return new RepositoryInspectionResult(false, fullPath, null, null, null, true, 0, 0, new List<string>());

    var inside = RunGitIn(fullPath, "rev-parse", "--is-inside-work-tree");
    if (inside.ExitCode != 0 || !string.Equals(inside.StdOut, "true", StringComparison.OrdinalIgnoreCase))
        return new RepositoryInspectionResult(false, fullPath, null, null, null, true, 0, 0, new List<string>());

    var branch = CurrentBranch(fullPath);
    var status = RunGitIn(fullPath, "status", "--porcelain");
    EnsureSuccess(status, $"Cannot inspect status in '{path}'.");

    var upstream = RunGitIn(fullPath, "rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}");
    int ahead = 0;
    int behind = 0;

    if (upstream.ExitCode == 0 && !string.IsNullOrWhiteSpace(upstream.StdOut))
    {
        var counts = RunGitIn(fullPath, "rev-list", "--left-right", "--count", $"{upstream.StdOut}...HEAD");
        if (counts.ExitCode == 0)
        {
            var parts = counts.StdOut.Split('\t', StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length == 2)
            {
                _ = int.TryParse(parts[1], out ahead);
                _ = int.TryParse(parts[0], out behind);
            }
        }
    }

    var head = ReadHeadCommit(fullPath);
    return new RepositoryInspectionResult(
        true,
        fullPath,
        branch,
        head.ShortCommit,
        head.Subject,
        string.IsNullOrWhiteSpace(status.StdOut),
        ahead,
        behind,
        SplitLines(status.StdOut));
}

static CommitResult BuildCommitResult(string? workingDirectory, bool amended)
{
    var head = ReadHeadCommit(workingDirectory);
    return head with { Amended = amended };
}

static CommitResult ReadHeadCommit(string? workingDirectory)
{
    var hash = RunGitIn(workingDirectory, "rev-parse", "--short", "HEAD");
    EnsureSuccess(hash, "Cannot read HEAD commit.");

    var subject = RunGitIn(workingDirectory, "log", "-1", "--pretty=%s");
    EnsureSuccess(subject, "Cannot read HEAD subject.");

    var stats = RunGitIn(workingDirectory, "show", "--stat", "--format=", "HEAD");
    EnsureSuccess(stats, "Cannot read HEAD stats.");

    return new CommitResult(hash.StdOut, subject.StdOut, SplitLines(stats.StdOut), false);
}

record GitCommandResult(int ExitCode, string StdOut, string StdErr);
record BranchResult(string Name);
record BranchExistsResult(string Name, bool Remote, bool Exists);
record CheckoutResult(string Branch, string Source, string Output);
record FetchResult(bool Fetched, bool All, string Output);
record PushResult(bool Pushed, bool Force, bool SetUpstream, string Output);
record PullResult(bool Pulled, bool Rebase, string Output);
record StatusResult(bool Clean, string Status, List<string> Entries);
record StashInfo(string Ref, string Message);
record StashSaveResult(string StashRef, string Message, bool AlreadyExisted);
record StashPopResult(string StashRef, bool Popped);
record StashListResult(int Count, List<StashInfo> Stashes);
record LogResult(int Count, List<string> Commits);
record DiffResult(string Target, int Ahead, int Behind, List<string> CommitsAhead, List<string> CommitsBehind);
record IntegrationResult(string Kind, string Target, bool Success, List<string> Conflicts, string Output, string Error);
record DiscardResult(bool Discarded);
record RemoteUrlResult(string Name, string Url);
record CommitResult(string ShortCommit, string Subject, List<string> Stats, bool Amended);
record StageResult(string Path, bool Staged);
record RepositoryInspectionResult(
    bool Exists,
    string Path,
    string? Branch,
    string? ShortCommit,
    string? Subject,
    bool Clean,
    int Ahead,
    int Behind,
    List<string> StatusEntries);
