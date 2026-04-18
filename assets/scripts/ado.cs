#!/usr/bin/dotnet run
#:package DotNetEnv@3.1.1
#:package System.CommandLine@2.0.0-beta4.22272.1
#:package YamlDotNet@16.3.0
#:property PublishAot=false
#:property PublishTrimmed=false

#nullable enable

using System.CommandLine;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using DotNetEnv;
using YamlDotNet.Serialization;
using YamlDotNet.Serialization.NamingConventions;

// Load .env from cwd, falling back to the script's own directory
var envPath = Path.Combine(Directory.GetCurrentDirectory(), ".env");
if (!File.Exists(envPath))
    envPath = Path.Combine(AppContext.BaseDirectory, ".env");
if (File.Exists(envPath))
    Env.Load(envPath);

var jsonOpts = new JsonSerializerOptions { WriteIndented = true };
var yaml     = new SerializerBuilder()
    .WithNamingConvention(CamelCaseNamingConvention.Instance)
    .Build();

// ── shared options ────────────────────────────────────────────────────────────

var formatOpt = new Option<string>("--format", () => "json", "Output format: json, yaml, markdown");

// ── wi get ────────────────────────────────────────────────────────────────────

var wiGetIdOpt = new Option<int>("--id", "Work item ID") { IsRequired = true };

var wiGetCmd = new Command("get", "Get a work item by ID") { wiGetIdOpt, formatOpt };

wiGetCmd.SetHandler(async (int id, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Fetching work item #{id}...");

    var resp = await http.GetAsync(V($"wit/workitems/{id}?$expand=all"));
    if (await ExitOnError(resp) != 0) return;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    var root      = doc.RootElement;
    var fields    = root.GetProperty("fields").Clone();
    JsonElement?  relations = root.TryGetProperty("relations", out var rel) ? rel.Clone() : null;

    Render(format, new WorkItemResult(id, fields, relations));
    Console.Error.WriteLine("Done.");

}, wiGetIdOpt, formatOpt);

// ── wi get-children ────────────────────────────────────────────────────────

var wiGetChildrenIdOpt = new Option<int>("--id", "Work item ID") { IsRequired = true };

var wiGetChildrenCmd = new Command("get-children", "Get child work items of a work item") { wiGetChildrenIdOpt, formatOpt };

wiGetChildrenCmd.SetHandler(async (int id, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Fetching children of work item #{id}...");

    var query = $"SELECT [System.Id], [System.Title], [System.WorkItemType], [System.State], [System.AssignedTo] FROM WorkItemLinks WHERE Source.Id = {id} AND LinkType = 'Child'";
    var wiqlBody = new StringContent(
        JsonSerializer.Serialize(new { query }),
        Encoding.UTF8, "application/json");

    var wiqlResp = await http.PostAsync($"wit/wiql?$top=500&api-version=7.1", wiqlBody);
    if (await ExitOnError(wiqlResp) != 0) return;

    using var wiqlDoc = await JsonDocument.ParseAsync(await wiqlResp.Content.ReadAsStreamAsync());
    var workItems = wiqlDoc.RootElement
        .GetProperty("workItems")
        .EnumerateArray()
        .Select(e => e.Clone())
        .ToList();

    Console.Error.WriteLine($"  WIQL returned {workItems.Count} child(ren).");

    if (workItems.Count == 0)
    {
        Render(format, new WorkItemsResult(0, []));
        Console.Error.WriteLine("Done. 0 child work item(s) returned.");
        return;
    }

    var batchTasks = workItems
        .Chunk(200)
        .Select(async batch =>
        {
            var csvIds = string.Join(",", batch.Select(e => e.GetProperty("id").GetInt32()));
            var batchResp = await http.GetAsync(V($"wit/workitems?ids={csvIds}&$expand=relations"));
            if (await ExitOnError(batchResp) != 0)
                throw new InvalidOperationException("Batch fetch failed.");

            using var batchDoc = await JsonDocument.ParseAsync(await batchResp.Content.ReadAsStreamAsync());
            return batchDoc.RootElement.GetProperty("value").EnumerateArray().Select(e => e.Clone()).ToList();
        })
        .ToList();

    List<JsonElement>[] batches;
    try
    {
        batches = await Task.WhenAll(batchTasks);
    }
    catch (InvalidOperationException)
    {
        return;
    }

    var allItems = batches.SelectMany(x => x).ToList();

    Render(format, new WorkItemsResult(allItems.Count, allItems));
    Console.Error.WriteLine($"Done. {allItems.Count} child work item(s) returned.");

}, wiGetChildrenIdOpt, formatOpt);

// ── wi create ─────────────────────────────────────────────────────────────────

var wiCreateTypeOpt        = new Option<string>("--type",        "Work item type (e.g. Bug, Task, \"User Story\")") { IsRequired = true };
var wiCreateTitleOpt       = new Option<string>("--title",       "Title")                                           { IsRequired = true };
var wiCreateDescOpt        = new Option<string?>("--description", () => null, "Description (HTML accepted)");
var wiCreateAssignedToOpt  = new Option<string?>("--assigned-to", () => null, "Assigned-to email or display name");
var wiCreateTagsOpt        = new Option<string?>("--tags",        () => null, "Semicolon-separated tags");

var wiCreateCmd = new Command("create", "Create a new work item")
    { wiCreateTypeOpt, wiCreateTitleOpt, wiCreateDescOpt, wiCreateAssignedToOpt, wiCreateTagsOpt, formatOpt };

wiCreateCmd.SetHandler(async (string type, string title, string? desc, string? assignedTo, string? tags, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Creating {type} '{title}'...");

    var body = BuildPatch(
        ("/fields/System.Title",       title),
        ("/fields/System.Description", desc),
        ("/fields/System.AssignedTo",  assignedTo),
        ("/fields/System.Tags",        tags)
    );

    var resp = await http.PostAsync(V($"wit/workitems/${Uri.EscapeDataString(type)}"), body);
    if (await ExitOnError(resp) != 0) return;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    var root      = doc.RootElement;
    var id        = root.GetProperty("id").GetInt32();
    var fields    = root.GetProperty("fields").Clone();

    Render(format, new WorkItemResult(id, fields, null));
    Console.Error.WriteLine($"Done. Work item #{id} created.");

}, wiCreateTypeOpt, wiCreateTitleOpt, wiCreateDescOpt, wiCreateAssignedToOpt, wiCreateTagsOpt, formatOpt);

// ── wi update ─────────────────────────────────────────────────────────────────

var wiUpdateIdOpt         = new Option<int>("--id",           "Work item ID")     { IsRequired = true };
var wiUpdateTitleOpt      = new Option<string?>("--title",       () => null, "New title");
var wiUpdateStateOpt      = new Option<string?>("--state",       () => null, "New state (e.g. Active, Closed)");
var wiUpdateDescOpt       = new Option<string?>("--description", () => null, "New description");
var wiUpdateAssignedToOpt = new Option<string?>("--assigned-to", () => null, "New assigned-to");
var wiUpdateTagsOpt       = new Option<string?>("--tags",        () => null, "New tags (semicolon-separated)");

var wiUpdateCmd = new Command("update", "Update an existing work item")
    { wiUpdateIdOpt, wiUpdateTitleOpt, wiUpdateStateOpt, wiUpdateDescOpt, wiUpdateAssignedToOpt, wiUpdateTagsOpt, formatOpt };

wiUpdateCmd.SetHandler(async (int id, string? title, string? state, string? desc, string? assignedTo, string? tags, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Updating work item #{id}...");

    var body = BuildPatch(
        ("/fields/System.Title",       title),
        ("/fields/System.State",       state),
        ("/fields/System.Description", desc),
        ("/fields/System.AssignedTo",  assignedTo),
        ("/fields/System.Tags",       tags)
    );

    var resp = await http.PatchAsync(V($"wit/workitems/{id}"), body);
    if (await ExitOnError(resp) != 0) return;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    var root      = doc.RootElement;
    var fields    = root.GetProperty("fields").Clone();
    JsonElement? relations = root.TryGetProperty("relations", out var rel) ? rel.Clone() : null;

    Render(format, new WorkItemResult(id, fields, relations));
    Console.Error.WriteLine($"Done. Work item #{id} updated.");

}, wiUpdateIdOpt, wiUpdateTitleOpt, wiUpdateStateOpt, wiUpdateDescOpt, wiUpdateAssignedToOpt, wiUpdateTagsOpt, formatOpt);

// ── wi link ────────────────────────────────────────────────────────────────

var wiLinkIdOpt = new Option<int>("--id", "Work item ID") { IsRequired = true };
var wiLinkRepoOpt = new Option<string?>("--repo", () => null, "Repository name or ID");
var wiLinkBranchOpt = new Option<string?>("--branch", () => null, "Branch name to link");
var wiLinkPrOpt = new Option<int?>("--pr", () => null, "PR ID to link");

var wiLinkCmd = new Command("link", "Ensure Branch/PR artifact links on a work item")
    { wiLinkIdOpt, wiLinkRepoOpt, wiLinkBranchOpt, wiLinkPrOpt, formatOpt };

wiLinkCmd.SetHandler(async (int id, string? repo, string? branch, int? prId, string format) =>
{
    if (branch is null && prId is null)
    {
        Console.Error.WriteLine("Error: at least one of --branch or --pr is required.");
        Environment.Exit(1);
        return;
    }

    if (repo is null)
    {
        Console.Error.WriteLine("Error: --repo is required when linking branch or PR artifacts.");
        Environment.Exit(1);
        return;
    }

    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Ensuring artifact links on work item #{id}...");

    var workItem = await GetWorkItem(http, id, expandRelations: true);
    if (workItem is null)
        return;

    var repoInfo = await GetRepository(http, repo);
    if (repoInfo is null)
        return;

    var projectId = repoInfo.Value.GetProperty("project").GetProperty("id").GetString()
        ?? throw new InvalidOperationException("Repository project id not found.");
    var repoId = repoInfo.Value.GetProperty("id").GetString()
        ?? throw new InvalidOperationException("Repository id not found.");

    var existingUrls = workItem.Value.TryGetProperty("relations", out var rels)
        ? rels.EnumerateArray()
            .Select(x => x.TryGetProperty("url", out var url) ? url.GetString() : null)
            .Where(x => !string.IsNullOrWhiteSpace(x))
            .Select(x => x!)
            .ToHashSet(StringComparer.OrdinalIgnoreCase)
        : new HashSet<string>(StringComparer.OrdinalIgnoreCase);

    var ops = new JsonArray();

    if (branch is not null)
    {
        var branchUrl = BranchArtifactUrl(projectId, repoId, branch);
        if (!existingUrls.Contains(branchUrl))
        {
            ops.Add(BuildRelationAddOperation("ArtifactLink", branchUrl, new JsonObject
            {
                ["name"] = "Branch"
            }));
        }
    }

    if (prId is not null)
    {
        var prUrl = PullRequestArtifactUrl(projectId, repoId, prId.Value);
        if (!existingUrls.Contains(prUrl))
        {
            ops.Add(BuildRelationAddOperation("ArtifactLink", prUrl, new JsonObject
            {
                ["name"] = "Pull Request"
            }));
        }
    }

    JsonElement updated;
    if (ops.Count == 0)
    {
        updated = workItem.Value;
    }
    else
    {
        var resp = await http.PatchAsync(
            V($"wit/workitems/{id}"),
            new StringContent(ops.ToJsonString(), Encoding.UTF8, "application/json-patch+json"));
        if (await ExitOnError(resp) != 0) return;

        using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
        updated = doc.RootElement.Clone();
    }

    var fields = updated.GetProperty("fields").Clone();
    JsonElement? relationsResult = updated.TryGetProperty("relations", out var rel) ? rel.Clone() : null;
    Render(format, new WorkItemResult(id, fields, relationsResult));
    Console.Error.WriteLine("Done.");
}, wiLinkIdOpt, wiLinkRepoOpt, wiLinkBranchOpt, wiLinkPrOpt, formatOpt);

// ── wi add-child ────────────────────────────────────────────────────────────

var wiAddChildParentIdOpt = new Option<int>("--parent-id", "Parent work item ID") { IsRequired = true };
var wiAddChildChildIdOpt = new Option<int>("--child-id", "Child work item ID") { IsRequired = true };

var wiAddChildCmd = new Command("add-child", "Ensure a child relation between two work items")
    { wiAddChildParentIdOpt, wiAddChildChildIdOpt, formatOpt };

wiAddChildCmd.SetHandler(async (int parentId, int childId, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Ensuring #{childId} is a child of #{parentId}...");

    var parent = await GetWorkItem(http, parentId, expandRelations: true);
    if (parent is null)
        return;

    var childUrl = WorkItemUrl(childId);
    var exists = parent.Value.TryGetProperty("relations", out var rels) &&
        rels.EnumerateArray().Any(x =>
            string.Equals(x.TryGetProperty("rel", out var rel) ? rel.GetString() : null, "System.LinkTypes.Hierarchy-Forward", StringComparison.Ordinal) &&
            string.Equals(x.TryGetProperty("url", out var url) ? url.GetString() : null, childUrl, StringComparison.OrdinalIgnoreCase));

    JsonElement updated;
    if (exists)
    {
        updated = parent.Value;
    }
    else
    {
        var patch = new JsonArray
        {
            BuildRelationAddOperation("System.LinkTypes.Hierarchy-Forward", childUrl)
        };

        var resp = await http.PatchAsync(
            V($"wit/workitems/{parentId}"),
            new StringContent(patch.ToJsonString(), Encoding.UTF8, "application/json-patch+json"));
        if (await ExitOnError(resp) != 0) return;

        using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
        updated = doc.RootElement.Clone();
    }

    var fields = updated.GetProperty("fields").Clone();
    JsonElement? relationsResult = updated.TryGetProperty("relations", out var rel) ? rel.Clone() : null;
    Render(format, new WorkItemResult(parentId, fields, relationsResult));
    Console.Error.WriteLine("Done.");
}, wiAddChildParentIdOpt, wiAddChildChildIdOpt, formatOpt);

// ── wi search ─────────────────────────────────────────────────────────────────

var wiSearchQueryOpt      = new Option<string>("--query",       "WIQL query string")                            { IsRequired = true };
var wiSearchMaxResultsOpt = new Option<int>   ("--max-results", () => 100, "Maximum number of results (1-500)");

var wiSearchCmd = new Command("search", "Search work items using WIQL") { wiSearchQueryOpt, wiSearchMaxResultsOpt, formatOpt };

wiSearchCmd.SetHandler(async (string query, int maxResults, string format) =>
{
    maxResults = Math.Clamp(maxResults, 1, 500);
    using var http = CreateHttpClient();
    Console.Error.WriteLine("Running WIQL query...");

    var wiqlBody = new StringContent(
        JsonSerializer.Serialize(new { query }),
        Encoding.UTF8, "application/json");

    var wiqlResp = await http.PostAsync($"wit/wiql?$top={maxResults}&api-version=7.1", wiqlBody);
    if (await ExitOnError(wiqlResp) != 0) return;

    using var wiqlDoc = await JsonDocument.ParseAsync(await wiqlResp.Content.ReadAsStreamAsync());
    var ids = wiqlDoc.RootElement
        .GetProperty("workItems")
        .EnumerateArray()
        .Select(e => e.GetProperty("id").GetInt32())
        .ToList();

    Console.Error.WriteLine($"  WIQL returned {ids.Count} IDs.");

    if (ids.Count == 0)
    {
        Render(format, new WorkItemsResult(0, []));
        Console.Error.WriteLine("Done. 0 work item(s) returned.");
        return;
    }

    var batchTasks = ids
        .Chunk(200)
        .Select(async batch =>
        {
            var csvIds = string.Join(",", batch);
            var batchResp = await http.GetAsync(V($"wit/workitems?ids={csvIds}&$expand=relations"));
            if (await ExitOnError(batchResp) != 0)
                throw new InvalidOperationException("Batch fetch failed.");

            using var batchDoc = await JsonDocument.ParseAsync(await batchResp.Content.ReadAsStreamAsync());
            return batchDoc.RootElement.GetProperty("value").EnumerateArray().Select(e => e.Clone()).ToList();
        })
        .ToList();

    List<JsonElement>[] batches;
    try
    {
        batches = await Task.WhenAll(batchTasks);
    }
    catch (InvalidOperationException)
    {
        return;
    }

    var allItems = batches.SelectMany(x => x).ToList();
    Console.Error.WriteLine($"  Fetched {allItems.Count} item(s) in {batches.Length} batch(es).");

    Render(format, new WorkItemsResult(allItems.Count, allItems));
    Console.Error.WriteLine($"Done. {allItems.Count} work item(s) returned.");

}, wiSearchQueryOpt, wiSearchMaxResultsOpt, formatOpt);

var wiCmd = new Command("wi", "Manage work items")
{
    wiGetCmd,
    wiGetChildrenCmd,
    wiCreateCmd,
    wiUpdateCmd,
    wiLinkCmd,
    wiAddChildCmd,
    wiSearchCmd
};

// ── pr get ─────────────────────────────────────────────────────────────────

var prGetRepoOpt = new Option<string>("--repo", "Repository name or ID") { IsRequired = true };
var prGetIdOpt   = new Option<int>   ("--id",  "Pull request ID")        { IsRequired = true };

var prGetCmd = new Command("get", "Get a pull request by ID") { prGetRepoOpt, prGetIdOpt, formatOpt };

prGetCmd.SetHandler(async (string repo, int id, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Fetching PR #{id} in '{repo}'...");

    var resp = await http.GetAsync(V($"git/repositories/{Uri.EscapeDataString(repo)}/pullrequests/{id}"));
    if (await ExitOnError(resp) != 0) return;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    var root = doc.RootElement;
    var prId = root.GetProperty("pullRequestId").GetInt32();

    Render(format, new PrResult(prId, root.Clone()));
    Console.Error.WriteLine("Done.");

}, prGetRepoOpt, prGetIdOpt, formatOpt);

// ── pr get-by-branch ─────────────────────────────────────────────────────

var prGetByBranchRepoOpt   = new Option<string>("--repo",   "Repository name or ID") { IsRequired = true };
var prGetByBranchSourceOpt = new Option<string>("--source", "Source branch name")    { IsRequired = true };
var prGetByBranchStatusOpt = new Option<string>("--status", () => "active", "PR status: active, abandoned, completed, all");

var prGetByBranchCmd = new Command("get-by-branch", "Find PR by source branch name")
    { prGetByBranchRepoOpt, prGetByBranchSourceOpt, prGetByBranchStatusOpt, formatOpt };

prGetByBranchCmd.SetHandler(async (string repo, string source, string status, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Searching PR by source branch '{source}' in '{repo}'...");

    var resp = await http.GetAsync(V(
        $"git/repositories/{Uri.EscapeDataString(repo)}/pullrequests" +
        $"?searchCriteria.sourceRefName={Uri.EscapeDataString(NormalizeRef(source))}" +
        $"&searchCriteria.status={Uri.EscapeDataString(status)}"));
    if (await ExitOnError(resp) != 0) return;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    var prs = doc.RootElement.GetProperty("value").EnumerateArray().Select(x => x.Clone()).ToList();

    var found = prs.FirstOrDefault(pr =>
        pr.TryGetProperty("sourceRefName", out var src) &&
        StripRefsHeads(src.GetString() ?? "") == source);

    if (found.ValueKind == JsonValueKind.Undefined)
    {
        Render(format, new PrResult(0, default));
        Console.Error.WriteLine("Done. No PR found for branch.");
        return;
    }

    var prId = found.TryGetProperty("pullRequestId", out var pid) ? pid.GetInt32() : 0;
    Render(format, new PrResult(prId, found.Clone()));
    Console.Error.WriteLine($"Done. Found PR #{prId}.");

}, prGetByBranchRepoOpt, prGetByBranchSourceOpt, prGetByBranchStatusOpt, formatOpt);

// ── pr list ────────────────────────────────────────────────────────────────

var prListRepoOpt = new Option<string>("--repo", "Repository name or ID") { IsRequired = true };
var prListSourceOpt = new Option<string?>("--source", () => null, "Optional source branch");
var prListStatusOpt = new Option<string>("--status", () => "active", "PR status: active, abandoned, completed, all");

var prListCmd = new Command("list", "List pull requests")
    { prListRepoOpt, prListSourceOpt, prListStatusOpt, formatOpt };

prListCmd.SetHandler(async (string repo, string? source, string status, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Listing PRs in '{repo}'...");

    var query = new StringBuilder($"git/repositories/{Uri.EscapeDataString(repo)}/pullrequests?searchCriteria.status={Uri.EscapeDataString(status)}");
    if (!string.IsNullOrWhiteSpace(source))
        query.Append($"&searchCriteria.sourceRefName={Uri.EscapeDataString(NormalizeRef(source))}");

    var resp = await http.GetAsync(V(query.ToString()));
    if (await ExitOnError(resp) != 0) return;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    var prs = doc.RootElement.GetProperty("value").EnumerateArray().Select(x => x.Clone()).ToList();
    Render(format, new PrsResult(prs.Count, prs));
    Console.Error.WriteLine($"Done. {prs.Count} PR(s) returned.");
}, prListRepoOpt, prListSourceOpt, prListStatusOpt, formatOpt);

// ── pr create ─────────────────────────────────────────────────────────────────

var prCreateRepoOpt   = new Option<string>("--repo",        "Repository name or ID")         { IsRequired = true };
var prCreateSourceOpt = new Option<string>("--source",      "Source branch name")             { IsRequired = true };
var prCreateTargetOpt = new Option<string>("--target",      "Target branch name")             { IsRequired = true };
var prCreateTitleOpt  = new Option<string>("--title",       "Pull request title")             { IsRequired = true };
var prCreateDescOpt       = new Option<string?>("--description",  () => null, "Pull request description");
var prCreateDraftOpt      = new Option<bool>  ("--draft",         "Create as draft pull request");
var prCreateWorkItemIdOpt = new Option<int?>  ("--work-item-id",  () => null, "Work item ID to link to this PR");

var prCreateCmd = new Command("create", "Create a pull request")
    { prCreateRepoOpt, prCreateSourceOpt, prCreateTargetOpt, prCreateTitleOpt, prCreateDescOpt, prCreateDraftOpt, prCreateWorkItemIdOpt, formatOpt };

prCreateCmd.SetHandler(async (string repo, string source, string target, string title, string? desc, bool draft, int? workItemId, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Creating PR '{title}' ({source} → {target}) in '{repo}'...");

    var body = new JsonObject
    {
        ["sourceRefName"] = NormalizeRef(source),
        ["targetRefName"] = NormalizeRef(target),
        ["title"]         = title,
        ["isDraft"]       = draft
    };
    if (desc is not null) body["description"] = desc;
    if (workItemId.HasValue)
    {
        body["workItemRefs"] = new JsonArray(new JsonObject
        {
            ["id"] = workItemId.Value.ToString()
        });
    }

    var resp = await http.PostAsync(
        V($"git/repositories/{Uri.EscapeDataString(repo)}/pullrequests"),
        new StringContent(body.ToJsonString(), Encoding.UTF8, "application/json"));
    if (await ExitOnError(resp) != 0) return;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    var root = doc.RootElement;
    var prId = root.GetProperty("pullRequestId").GetInt32();

    Render(format, new PrResult(prId, root.Clone()));
    Console.Error.WriteLine($"Done. PR #{prId} created.");

}, prCreateRepoOpt, prCreateSourceOpt, prCreateTargetOpt, prCreateTitleOpt, prCreateDescOpt, prCreateDraftOpt, prCreateWorkItemIdOpt, formatOpt);

// ── pr update ─────────────────────────────────────────────────────────────────

var prUpdateRepoOpt   = new Option<string>("--repo",         "Repository name or ID") { IsRequired = true };
var prUpdateIdOpt     = new Option<int>   ("--id",           "Pull request ID")        { IsRequired = true };
var prUpdateTitleOpt   = new Option<string?>("--title",        () => null, "New title");
var prUpdateDescOpt    = new Option<string?>("--description",  () => null, "New description");
var prUpdateStatusOpt  = new Option<string?>("--status",       () => null, "New status: active, abandoned, completed");
var prUpdatePublishOpt = new Option<bool>   ("--publish",      "Remove draft status (publish the PR)");
var prUpdateReviewerOpt = new Option<string[]>("--add-reviewer", "Reviewer unique name/email") { AllowMultipleArgumentsPerToken = true };

var prUpdateCmd = new Command("update", "Update a pull request")
    { prUpdateRepoOpt, prUpdateIdOpt, prUpdateTitleOpt, prUpdateDescOpt, prUpdateStatusOpt, prUpdatePublishOpt, prUpdateReviewerOpt, formatOpt };

prUpdateCmd.SetHandler(async (string repo, int id, string? title, string? desc, string? status, bool publish, string[] reviewers, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Updating PR #{id} in '{repo}'...");

    var body = new JsonObject();
    if (title   is not null) body["title"]       = title;
    if (desc    is not null) body["description"] = desc;
    if (status  is not null) body["status"]      = status;
    if (publish)             body["isDraft"]     = false;

    JsonElement root;
    if (body.Count > 0)
    {
        var req = new HttpRequestMessage(HttpMethod.Patch,
            V($"git/repositories/{Uri.EscapeDataString(repo)}/pullrequests/{id}"))
        {
            Content = new StringContent(body.ToJsonString(), Encoding.UTF8, "application/json")
        };

        var resp = await http.SendAsync(req);
        if (await ExitOnError(resp) != 0) return;

        using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
        root = doc.RootElement.Clone();
    }
    else
    {
        var resp = await http.GetAsync(V($"git/repositories/{Uri.EscapeDataString(repo)}/pullrequests/{id}"));
        if (await ExitOnError(resp) != 0) return;

        using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
        root = doc.RootElement.Clone();
    }

    if (reviewers.Length > 0)
    {
        foreach (var reviewer in reviewers.Where(x => !string.IsNullOrWhiteSpace(x)))
        {
            var reviewerReq = new HttpRequestMessage(HttpMethod.Put,
                V($"git/repositories/{Uri.EscapeDataString(repo)}/pullrequests/{id}/reviewers/{Uri.EscapeDataString(reviewer)}"))
            {
                Content = new StringContent(new JsonObject { ["vote"] = 0 }.ToJsonString(), Encoding.UTF8, "application/json")
            };
            var reviewerResp = await http.SendAsync(reviewerReq);
            if (await ExitOnError(reviewerResp) != 0) return;
        }

        var refreshed = await http.GetAsync(V($"git/repositories/{Uri.EscapeDataString(repo)}/pullrequests/{id}"));
        if (await ExitOnError(refreshed) != 0) return;

        using var doc = await JsonDocument.ParseAsync(await refreshed.Content.ReadAsStreamAsync());
        root = doc.RootElement.Clone();
    }

    var prId = root.GetProperty("pullRequestId").GetInt32();
    Render(format, new PrResult(prId, root));
    Console.Error.WriteLine($"Done. PR #{prId} updated.");

}, prUpdateRepoOpt, prUpdateIdOpt, prUpdateTitleOpt, prUpdateDescOpt, prUpdateStatusOpt, prUpdatePublishOpt, prUpdateReviewerOpt, formatOpt);

// ── pr merge ───────────────────────────────────────────────────────────────

var prMergeRepoOpt = new Option<string>("--repo", "Repository name or ID") { IsRequired = true };
var prMergeIdOpt = new Option<int>("--id", "Pull request ID") { IsRequired = true };
var prMergeStrategyOpt = new Option<string>("--strategy", () => "squash", "Merge strategy: squash, rebase, rebaseMerge, noFastForward");
var prMergeDeleteSourceOpt = new Option<bool>("--delete-source-branch", "Delete the source branch after merge");
var prMergeBypassOpt = new Option<bool>("--bypass-policy", "Bypass policies");

var prMergeCmd = new Command("merge", "Complete a pull request")
    { prMergeRepoOpt, prMergeIdOpt, prMergeStrategyOpt, prMergeDeleteSourceOpt, prMergeBypassOpt, formatOpt };

prMergeCmd.SetHandler(async (string repo, int id, string strategy, bool deleteSourceBranch, bool bypassPolicy, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Completing PR #{id} in '{repo}'...");

    var currentResp = await http.GetAsync(V($"git/repositories/{Uri.EscapeDataString(repo)}/pullrequests/{id}"));
    if (await ExitOnError(currentResp) != 0) return;

    using var currentDoc = await JsonDocument.ParseAsync(await currentResp.Content.ReadAsStreamAsync());
    var current = currentDoc.RootElement.Clone();

    var body = new JsonObject
    {
        ["status"] = "completed",
        ["completionOptions"] = new JsonObject
        {
            ["mergeStrategy"] = strategy,
            ["deleteSourceBranch"] = deleteSourceBranch,
            ["bypassPolicy"] = bypassPolicy
        }
    };

    if (current.TryGetProperty("lastMergeSourceCommit", out var sourceCommit) &&
        sourceCommit.TryGetProperty("commitId", out var commitId))
    {
        body["lastMergeSourceCommit"] = new JsonObject
        {
            ["commitId"] = commitId.GetString()
        };
    }

    var req = new HttpRequestMessage(HttpMethod.Patch,
        V($"git/repositories/{Uri.EscapeDataString(repo)}/pullrequests/{id}"))
    {
        Content = new StringContent(body.ToJsonString(), Encoding.UTF8, "application/json")
    };

    var resp = await http.SendAsync(req);
    if (await ExitOnError(resp) != 0) return;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    var root = doc.RootElement.Clone();
    Render(format, new PrResult(id, root));
    Console.Error.WriteLine("Done.");
}, prMergeRepoOpt, prMergeIdOpt, prMergeStrategyOpt, prMergeDeleteSourceOpt, prMergeBypassOpt, formatOpt);

var prCmd = new Command("pr", "Manage pull requests")
{
    prGetCmd,
    prGetByBranchCmd,
    prListCmd,
    prCreateCmd,
    prUpdateCmd,
    prMergeCmd
};

// ── refs create ────────────────────────────────────────────────────────────

var refsCreateRepoOpt   = new Option<string>("--repo",   "Repository name or ID") { IsRequired = true };
var refsCreateBranchOpt = new Option<string>("--branch", "Branch name to create")   { IsRequired = true };
var refsCreateSourceOpt = new Option<string?>("--source", () => null, "Source branch (default: target branch's HEAD)");

var refsCreateCmd = new Command("create", "Create a new remote branch") { refsCreateRepoOpt, refsCreateBranchOpt, refsCreateSourceOpt, formatOpt };

refsCreateCmd.SetHandler(async (string repo, string branch, string? source, string format) =>
{
    using var http = CreateHttpClient();
    var normalizedBranch = NormalizeRef(branch);
    Console.Error.WriteLine($"Creating branch '{branch}' in '{repo}'...");

    var checkResp = await http.GetAsync(V($"git/repositories/{Uri.EscapeDataString(repo)}/refs?filter={Uri.EscapeDataString(normalizedBranch)}"));
    if (await ExitOnError(checkResp) != 0) return;

    using var checkDoc = await JsonDocument.ParseAsync(await checkResp.Content.ReadAsStreamAsync());
    var existingRefs = checkDoc.RootElement.GetProperty("value").EnumerateArray().ToList();

    if (existingRefs.Count > 0)
    {
        Console.Error.WriteLine($"Branch '{branch}' already exists.");
        Render(format, new RefsResult(0, new List<JsonElement> { existingRefs[0].Clone() }));
        return;
    }

    string targetSha;
    if (source is not null)
    {
        var sourceRef = NormalizeRef(source);
        var sourceResp = await http.GetAsync(V($"git/repositories/{Uri.EscapeDataString(repo)}/refs?filter={Uri.EscapeDataString(sourceRef)}"));
        if (await ExitOnError(sourceResp) != 0) return;

        using var sourceDoc = await JsonDocument.ParseAsync(await sourceResp.Content.ReadAsStreamAsync());
        var sourceList = sourceDoc.RootElement.GetProperty("value").EnumerateArray().ToList();
        if (sourceList.Count == 0)
        {
            Console.Error.WriteLine($"Error: Source branch '{source}' not found.");
            return;
        }
        targetSha = sourceList[0].GetProperty("objectId").GetString() ?? throw new InvalidOperationException("Could not resolve objectId for source.");
    }
    else
    {
        var defaultBranch = await GetDefaultBranch(http, repo);
        var targetResp = await http.GetAsync(V($"git/repositories/{Uri.EscapeDataString(repo)}/refs?filter={Uri.EscapeDataString(defaultBranch)}"));
        if (await ExitOnError(targetResp) != 0) return;

        using var targetDoc = await JsonDocument.ParseAsync(await targetResp.Content.ReadAsStreamAsync());
        var targetList = targetDoc.RootElement.GetProperty("value").EnumerateArray().ToList();
        if (targetList.Count == 0)
        {
            Console.Error.WriteLine($"Error: Default branch '{defaultBranch}' not found.");
            return;
        }
        targetSha = targetList[0].GetProperty("objectId").GetString() ?? throw new InvalidOperationException("Could not resolve objectId for target.");
    }

    var newRefBody = new JsonArray(new JsonObject
    {
        ["name"]        = normalizedBranch,
        ["newObjectId"] = targetSha,
        ["oldObjectId"] = "0000000000000000000000000000000000000000"
    });

    var createResp = await http.PostAsync(
        V($"git/repositories/{Uri.EscapeDataString(repo)}/refs"),
        new StringContent(newRefBody.ToJsonString(), Encoding.UTF8, "application/json"));
    if (await ExitOnError(createResp) != 0) return;

    using var createDoc = await JsonDocument.ParseAsync(await createResp.Content.ReadAsStreamAsync());
    var createdRef = createDoc.RootElement.GetProperty("value").EnumerateArray().First().Clone();

    Render(format, new RefsResult(1, new List<JsonElement> { createdRef }));
    Console.Error.WriteLine($"Done. Branch '{branch}' created.");

}, refsCreateRepoOpt, refsCreateBranchOpt, refsCreateSourceOpt, formatOpt);

// ── refs exists ────────────────────────────────────────────────────────────────

var refsExistsRepoOpt  = new Option<string>("--repo",  "Repository name or ID") { IsRequired = true };
var refsExistsNameOpt = new Option<string>("--name", "Branch name")         { IsRequired = true };

var refsExistsCmd = new Command("exists", "Check if a remote branch exists") { refsExistsRepoOpt, refsExistsNameOpt };

refsExistsCmd.SetHandler(async (string repo, string name) =>
{
    using var http = CreateHttpClient();
    var normalizedRef = NormalizeRef(name);
    Console.Error.WriteLine($"Checking if branch '{name}' exists in '{repo}'...");

    var resp = await http.GetAsync(V($"git/repositories/{Uri.EscapeDataString(repo)}/refs?filter={Uri.EscapeDataString(normalizedRef)}"));
    if (await ExitOnError(resp) != 0) return;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    var refs = doc.RootElement.GetProperty("value").EnumerateArray().ToList();

    if (refs.Count > 0)
    {
        Console.Error.WriteLine("Branch exists.");
        Environment.Exit(0);
    }
    else
    {
        Console.Error.WriteLine("Branch does not exist.");
        Environment.Exit(1);
    }

}, refsExistsRepoOpt, refsExistsNameOpt);

var refsCmd = new Command("refs", "Manage git refs (branches)")
{
    refsCreateCmd,
    refsExistsCmd
};

// ── repo get ───────────────────────────────────────────────────────────────

var repoGetRepoOpt = new Option<string>("--repo", "Repository name or ID") { IsRequired = true };
var repoGetCmd = new Command("get", "Get repository details") { repoGetRepoOpt, formatOpt };

repoGetCmd.SetHandler(async (string repo, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Fetching repository '{repo}'...");

    var repoInfo = await GetRepository(http, repo);
    if (repoInfo is null)
        return;

    Render(format, new RepositoryResult(repoInfo.Value));
    Console.Error.WriteLine("Done.");
}, repoGetRepoOpt, formatOpt);

var repoCmd = new Command("repo", "Manage repositories")
{
    repoGetCmd
};

// ── pipeline run ───────────────────────���─���────────────────────────────────────

var pipelineRunIdOpt     = new Option<int>   ("--id",     "Pipeline definition ID") { IsRequired = true };
var pipelineRunBranchOpt = new Option<string?>("--branch", () => null, "Branch to run (defaults to pipeline default)");

var pipelineRunCmd = new Command("run", "Trigger a pipeline run") { pipelineRunIdOpt, pipelineRunBranchOpt, formatOpt };

pipelineRunCmd.SetHandler(async (int id, string? branch, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Triggering pipeline #{id}{(branch is not null ? $" on '{branch}'" : "")}...");

    JsonObject bodyObj;
    if (branch is not null)
    {
        bodyObj = new JsonObject
        {
            ["resources"] = new JsonObject
            {
                ["repositories"] = new JsonObject
                {
                    ["self"] = new JsonObject { ["refName"] = NormalizeRef(branch) }
                }
            }
        };
    }
    else
    {
        bodyObj = new JsonObject();
    }

    var resp = await http.PostAsync(
        V($"pipelines/{id}/runs"),
        new StringContent(bodyObj.ToJsonString(), Encoding.UTF8, "application/json"));
    if (await ExitOnError(resp) != 0) return;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    var root  = doc.RootElement;
    var runId = root.GetProperty("id").GetInt32();

    Render(format, new PipelineRunResult(id, runId, root.Clone()));
    Console.Error.WriteLine($"Done. Pipeline run #{runId} started.");

}, pipelineRunIdOpt, pipelineRunBranchOpt, formatOpt);

// ── pipeline info ─────────────────────────────────────────────────────────────

var pipelineInfoIdOpt    = new Option<int>("--id",     "Pipeline definition ID") { IsRequired = true };
var pipelineInfoRunIdOpt = new Option<int>("--run-id", "Pipeline run ID")         { IsRequired = true };

var pipelineInfoCmd = new Command("info", "Get pipeline run details") { pipelineInfoIdOpt, pipelineInfoRunIdOpt, formatOpt };

pipelineInfoCmd.SetHandler(async (int id, int runId, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Fetching pipeline #{id} run #{runId}...");

    var resp = await http.GetAsync(V($"pipelines/{id}/runs/{runId}"));
    if (await ExitOnError(resp) != 0) return;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());

    Render(format, new PipelineRunResult(id, runId, doc.RootElement.Clone()));
    Console.Error.WriteLine("Done.");

}, pipelineInfoIdOpt, pipelineInfoRunIdOpt, formatOpt);

// ── pipeline get ────────────────────────────────────────────────────────────

var pipelineGetRunIdOpt = new Option<int>("--run-id", "Build/run ID") { IsRequired = true };

var pipelineGetCmd = new Command("get", "Get a pipeline/build run by run ID")
{
    pipelineGetRunIdOpt, formatOpt
};

pipelineGetCmd.SetHandler(async (int runId, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Fetching run #{runId}...");

    var resp = await http.GetAsync(V($"build/builds/{runId}"));
    if (await ExitOnError(resp) != 0) return;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    Render(format, new BuildRunResult(runId, doc.RootElement.Clone()));
    Console.Error.WriteLine("Done.");
}, pipelineGetRunIdOpt, formatOpt);

// ── pipeline list ─────────────────────────────────────────────────────────────

var pipelineListSearchOpt = new Option<string?>("--search", () => null, "Filter pipelines by name (case-insensitive substring)");

var pipelineListCmd = new Command("list", "List pipeline definitions") { pipelineListSearchOpt, formatOpt };

pipelineListCmd.SetHandler(async (string? search, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine("Fetching pipeline list...");

    var resp = await http.GetAsync(V("pipelines?$top=500"));
    if (await ExitOnError(resp) != 0) return;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    var pipelines = doc.RootElement.GetProperty("value").EnumerateArray().Select(e => e.Clone()).ToList();

    if (search is not null)
        pipelines = pipelines
            .Where(e => e.TryGetProperty("name", out var n) &&
                        (n.GetString() ?? "").Contains(search, StringComparison.OrdinalIgnoreCase))
            .ToList();

    Render(format, new PipelinesResult(pipelines.Count, pipelines));
    Console.Error.WriteLine($"Done. {pipelines.Count} pipeline(s) returned.");

}, pipelineListSearchOpt, formatOpt);

// ── pipeline latest ────────────────────────────────────────────────────────

var pipelineLatestBranchOpt = new Option<string>("--branch", "Branch name") { IsRequired = true };
var pipelineLatestIdOpt = new Option<int?>("--id", () => null, "Optional pipeline definition ID");

var pipelineLatestCmd = new Command("latest", "Get the latest build/run for a branch")
{
    pipelineLatestBranchOpt, pipelineLatestIdOpt, formatOpt
};

pipelineLatestCmd.SetHandler(async (string branch, int? id, string format) =>
{
    using var http = CreateHttpClient();
    Console.Error.WriteLine($"Fetching latest run for '{branch}'...");

    var query = new StringBuilder(
        $"build/builds?branchName={Uri.EscapeDataString(NormalizeRef(branch))}&$top=1&queryOrder=queueTimeDescending");
    if (id.HasValue)
        query.Append($"&definitions={id.Value}");

    var resp = await http.GetAsync(V(query.ToString()));
    if (await ExitOnError(resp) != 0) return;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    var runs = doc.RootElement.GetProperty("value").EnumerateArray().Select(x => x.Clone()).ToList();
    var run = runs.FirstOrDefault();

    if (run.ValueKind == JsonValueKind.Undefined)
    {
        Render(format, new BuildRunsResult(0, []));
        Console.Error.WriteLine("Done. No runs found.");
        return;
    }

    Render(format, new BuildRunResult(run.GetProperty("id").GetInt32(), run));
    Console.Error.WriteLine("Done.");
}, pipelineLatestBranchOpt, pipelineLatestIdOpt, formatOpt);

var pipelineCmd = new Command("pipeline", "Manage pipelines")
{
    pipelineRunCmd,
    pipelineInfoCmd,
    pipelineGetCmd,
    pipelineListCmd,
    pipelineLatestCmd
};

// ── workflow start ────────────────────────────────────────────────────────────

var wfSonarProjectOpt = new Option<string> ("--sonar-project", "SonarQube project key")                               { IsRequired = true };
var wfRepoOpt         = new Option<string> ("--repo",          "ADO repository name")                                 { IsRequired = true };
var wfWiTitleOpt      = new Option<string> ("--wi-title",      "Work item title")                                     { IsRequired = true };
var wfWiTypeOpt       = new Option<string> ("--wi-type",       () => "Task", "Work item type (default: Task)");
var wfTargetOpt       = new Option<string> ("--target",        () => "main", "PR target branch (default: main)");
var wfMaxIssuesOpt    = new Option<int>    ("--max-issues",    () => 50,     "Max SonarQube issues to include (default: 50)");

var wfStartCmd = new Command("start", "Run the full developer workflow: fetch Sonar issues → create WI → branch → draft PR → set Active")
    { wfSonarProjectOpt, wfRepoOpt, wfWiTitleOpt, wfWiTypeOpt, wfTargetOpt, wfMaxIssuesOpt, formatOpt };

wfStartCmd.SetHandler(async (string sonarProject, string repo, string wiTitle, string wiType, string target, int maxIssues, string format) =>
{
    // ── 1. Fetch SonarQube issues ────────────────────────────────────────────
    Console.Error.WriteLine($"[1/5] Fetching SonarQube issues for '{sonarProject}'...");

    var sonarUrl   = Environment.GetEnvironmentVariable("SONAR_URL")?.TrimEnd('/')
        ?? throw new InvalidOperationException("SONAR_URL environment variable is not set.");
    var sonarToken = Environment.GetEnvironmentVariable("SONAR_TOKEN")
        ?? throw new InvalidOperationException("SONAR_TOKEN environment variable is not set.");

    using var sonar = new HttpClient { BaseAddress = new Uri(sonarUrl) };
    var sonarEncoded = Convert.ToBase64String(Encoding.UTF8.GetBytes($"{sonarToken}:"));
    sonar.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Basic", sonarEncoded);

    var sonarResp = await sonar.GetAsync(
        $"/api/issues/search?componentKeys={Uri.EscapeDataString(sonarProject)}&ps={Math.Min(maxIssues, 100)}&p=1&resolved=false");
    if (await ExitOnError(sonarResp) != 0) return;

    using var sonarDoc  = await JsonDocument.ParseAsync(await sonarResp.Content.ReadAsStreamAsync());
    var issues = sonarDoc.RootElement.GetProperty("issues").EnumerateArray().Select(e => e.Clone()).ToList();
    Console.Error.WriteLine($"  Got {issues.Count} issue(s).");

    // ── 2. Build WI description from issues ──────────────────────────────────
    var descSb = new StringBuilder();
    descSb.Append($"<h2>SonarQube Issues — {sonarProject}</h2>");
    if (issues.Count == 0)
    {
        descSb.Append("<p><em>No open issues found.</em></p>");
    }
    else
    {
        descSb.Append("<ul>");
        foreach (var issue in issues)
        {
            var severity  = issue.TryGetProperty("severity",  out var sv) ? sv.GetString() : "";
            var message   = issue.TryGetProperty("message",   out var ms) ? ms.GetString() : "";
            var component = issue.TryGetProperty("component", out var cp) ? cp.GetString() : "";
            descSb.Append($"<li><strong>{severity}</strong>: {message} <em>({component})</em></li>");
        }
        descSb.Append("</ul>");
    }

    // ── 3. Create work item ───────────────────────────────────────────────────
    Console.Error.WriteLine($"[2/5] Creating {wiType} '{wiTitle}'...");

    using var ado = CreateHttpClient();

    var wiBody = BuildPatch(
        ("/fields/System.Title",       wiTitle),
        ("/fields/System.Description", descSb.ToString()),
        ("/fields/System.Tags",        $"sonarqube;{sonarProject}")
    );
    var wiResp = await ado.PostAsync(V($"wit/workitems/${Uri.EscapeDataString(wiType)}"), wiBody);
    if (await ExitOnError(wiResp) != 0) return;

    using var wiDoc = await JsonDocument.ParseAsync(await wiResp.Content.ReadAsStreamAsync());
    var wiId = wiDoc.RootElement.GetProperty("id").GetInt32();
    Console.Error.WriteLine($"  Work item #{wiId} created.");

    // ── 4. Create branch via ADO Git Refs API ─────────────────────────────────
    var slug   = System.Text.RegularExpressions.Regex.Replace(wiTitle.ToLowerInvariant(), @"[^a-z0-9]+", "-").Trim('-');
    if (slug.Length > 40) slug = slug[..40].TrimEnd('-');
    var branch = $"feature/{wiId}-{slug}";

    Console.Error.WriteLine($"[3/5] Creating branch '{branch}'...");

    var refsResp = await ado.GetAsync(V($"git/repositories/{Uri.EscapeDataString(repo)}/refs?filter=heads/{Uri.EscapeDataString(target)}"));
    if (await ExitOnError(refsResp) != 0) return;

    using var refsDoc = await JsonDocument.ParseAsync(await refsResp.Content.ReadAsStreamAsync());
    var targetSha = refsDoc.RootElement.GetProperty("value").EnumerateArray().First().GetProperty("objectId").GetString()
        ?? throw new InvalidOperationException($"Could not resolve objectId for '{target}'.");

    var newRefBody = new JsonArray(new JsonObject
    {
        ["name"]        = $"refs/heads/{branch}",
        ["newObjectId"] = targetSha,
        ["oldObjectId"] = "0000000000000000000000000000000000000000"
    });
    var newRefResp = await ado.PostAsync(
        V($"git/repositories/{Uri.EscapeDataString(repo)}/refs"),
        new StringContent(newRefBody.ToJsonString(), Encoding.UTF8, "application/json"));
    if (await ExitOnError(newRefResp) != 0) return;
    Console.Error.WriteLine($"  Branch '{branch}' created.");

    // ── 5. Create draft PR linked to WI ──────────────────────────────────────
    Console.Error.WriteLine($"[4/5] Creating draft PR...");

    var prBody = new JsonObject
    {
        ["sourceRefName"] = $"refs/heads/{branch}",
        ["targetRefName"] = NormalizeRef(target),
        ["title"]         = wiTitle,
        ["isDraft"]       = true,
        ["description"]   = $"Closes #{wiId}.",
        ["workItemRefs"]  = new JsonArray(new JsonObject { ["id"] = wiId.ToString() })
    };
    var prResp = await ado.PostAsync(
        V($"git/repositories/{Uri.EscapeDataString(repo)}/pullrequests"),
        new StringContent(prBody.ToJsonString(), Encoding.UTF8, "application/json"));
    if (await ExitOnError(prResp) != 0) return;

    using var prDoc = await JsonDocument.ParseAsync(await prResp.Content.ReadAsStreamAsync());
    var prId = prDoc.RootElement.GetProperty("pullRequestId").GetInt32();
    Console.Error.WriteLine($"  Draft PR #{prId} created.");

    // ── 6. Set WI to Active ─────────────────��─────────────────────────────────
    Console.Error.WriteLine($"[5/5] Setting WI #{wiId} to Active...");

    var activeBody = BuildPatch(("/fields/System.State", "Active"));
    var activeResp = await ado.PatchAsync(V($"wit/workitems/{wiId}"), activeBody);
    if (await ExitOnError(activeResp) != 0) return;
    Console.Error.WriteLine("  Done.");

    // ── Output summary ───────────────────────────────────────────────────────
    Render(format, new WorkflowResult(wiId, branch, prId, issues.Count));
    Console.Error.WriteLine($"Workflow complete. WI #{wiId} | Branch: {branch} | PR #{prId}");

}, wfSonarProjectOpt, wfRepoOpt, wfWiTitleOpt, wfWiTypeOpt, wfTargetOpt, wfMaxIssuesOpt, formatOpt);

var workflowCmd = new Command("workflow", "Automated developer workflows") { wfStartCmd };

// ── root ──────────────────────────────────────────────────────────────────────

var rootCmd = new RootCommand("Azure DevOps CLI utilities")
{
    wiCmd,
    prCmd,
    refsCmd,
    repoCmd,
    pipelineCmd,
    workflowCmd
};
return await rootCmd.InvokeAsync(args);

// ── local functions ───────────────────────────────────────────────────────────

void Render(string format, object data)
{
    var output = format.ToLowerInvariant() switch
    {
        "yaml"     => yaml.Serialize(ToYamlObject(data)),
        "markdown" => data switch
        {
            WorkItemResult    r => RenderWorkItemMarkdown(r),
            WorkItemsResult   r => RenderWorkItemsMarkdown(r),
            PrResult          r => RenderPrMarkdown(r),
            PrsResult         r => RenderPrsMarkdown(r),
            RefsResult        r => RenderRefsMarkdown(r),
            RepositoryResult  r => RenderRepositoryMarkdown(r),
            PipelineRunResult r => RenderPipelineRunMarkdown(r),
            BuildRunResult    r => RenderBuildRunMarkdown(r),
            BuildRunsResult   r => RenderBuildRunsMarkdown(r),
            PipelinesResult   r => RenderPipelinesMarkdown(r),
            WorkflowResult    r => RenderWorkflowMarkdown(r),
            _                 => throw new NotSupportedException($"No markdown renderer for {data.GetType().Name}")
        },
        _ => JsonSerializer.Serialize(data, jsonOpts)
    };
    Console.WriteLine(output);
}

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
    WorkItemResult r => new Dictionary<string, object?> {
        ["id"]        = r.Id,
        ["fields"]    = ToYamlObject(r.Fields),
        ["relations"] = r.Relations.HasValue ? ToYamlObject(r.Relations.Value) : null
    },
    WorkItemsResult r => new Dictionary<string, object?> {
        ["count"] = r.Count,
        ["items"] = r.Items.Select(e => ToYamlObject(e)).ToList()
    },
    PrResult r => new Dictionary<string, object?> {
        ["prId"] = r.PrId,
        ["pr"]   = r.Pr.ValueKind != JsonValueKind.Undefined ? ToYamlObject(r.Pr) : null
    },
    PrsResult r => new Dictionary<string, object?> {
        ["count"] = r.Count,
        ["prs"] = r.Prs.Select(e => ToYamlObject(e)).ToList()
    },
    RefsResult r => new Dictionary<string, object?> {
        ["count"] = r.Count,
        ["refs"]  = r.Refs.Select(e => ToYamlObject(e)).ToList()
    },
    RepositoryResult r => new Dictionary<string, object?> {
        ["repository"] = ToYamlObject(r.Repository)
    },
    PipelineRunResult r => new Dictionary<string, object?> {
        ["pipelineId"] = r.PipelineId,
        ["runId"]      = r.RunId,
        ["run"]        = ToYamlObject(r.Run)
    },
    BuildRunResult r => new Dictionary<string, object?> {
        ["runId"] = r.RunId,
        ["run"] = ToYamlObject(r.Run)
    },
    BuildRunsResult r => new Dictionary<string, object?> {
        ["count"] = r.Count,
        ["runs"] = r.Runs.Select(e => ToYamlObject(e)).ToList()
    },
    PipelinesResult r => new Dictionary<string, object?> {
        ["count"]     = r.Count,
        ["pipelines"] = r.Pipelines.Select(e => ToYamlObject(e)).ToList()
    },
    WorkflowResult r => new Dictionary<string, object?> {
        ["workItemId"] = r.WorkItemId,
        ["branch"]     = r.Branch,
        ["prId"]       = r.PrId,
        ["issueCount"] = r.IssueCount
    },
    _ => value
};

string RenderWorkItemMarkdown(WorkItemResult r)
{
    var sb = new StringBuilder();
    var f  = r.Fields;

    string Field(string name)
        => f.TryGetProperty(name, out var v) ? ExtractString(v) : "—";

    sb.AppendLine($"## Work Item #{r.Id} — {Field("System.Title")}");
    sb.AppendLine();
    sb.AppendLine("| Field | Value |");
    sb.AppendLine("|-------|-------|");
    sb.AppendLine($"| Type        | {Field("System.WorkItemType")} |");
    sb.AppendLine($"| State       | {Field("System.State")} |");
    sb.AppendLine($"| Assigned To | {Field("System.AssignedTo")} |");
    sb.AppendLine($"| Tags        | {Field("System.Tags")} |");
    sb.AppendLine();

    var desc = Field("System.Description");
    sb.AppendLine("### Description");
    sb.AppendLine();
    sb.AppendLine(desc == "—" ? "_No description._" : desc);

    return sb.ToString();
}

string RenderWorkItemsMarkdown(WorkItemsResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine("# Work Item Search Results");
    sb.AppendLine();
    sb.AppendLine($"**Count:** {r.Count}");
    sb.AppendLine();

    if (r.Count == 0)
    {
        sb.AppendLine("_No results found._");
        return sb.ToString();
    }

    sb.AppendLine("| ID | Type | State | Title | Assigned To |");
    sb.AppendLine("|----|------|-------|-------|-------------|");

    foreach (var item in r.Items)
    {
        var id = item.TryGetProperty("id", out var idEl) ? idEl.GetInt32().ToString() : "?";
        string ItemField(string name)
        {
            if (!item.TryGetProperty("fields", out var fields)) return "—";
            if (!fields.TryGetProperty(name, out var v)) return "—";
            return ExtractString(v);
        }
        sb.AppendLine($"| {id} | {ItemField("System.WorkItemType")} | {ItemField("System.State")} | {ItemField("System.Title")} | {ItemField("System.AssignedTo")} |");
    }

    return sb.ToString();
}

string RenderPrMarkdown(PrResult r)
{
    var sb = new StringBuilder();

    if (r.Pr.ValueKind == JsonValueKind.Undefined || r.PrId == 0)
    {
        sb.AppendLine("## Pull Request");
        sb.AppendLine();
        sb.AppendLine("_No PR found._");
        return sb.ToString();
    }

    var pr = r.Pr;

    string PrField(string name)
        => pr.TryGetProperty(name, out var v) ? ExtractString(v) : "—";

    var title  = PrField("title");
    var status = PrField("status");
    var source = StripRefsHeads(PrField("sourceRefName"));
    var target = StripRefsHeads(PrField("targetRefName"));
    var desc   = PrField("description");

    var createdBy = pr.TryGetProperty("createdBy", out var cb) && cb.TryGetProperty("displayName", out var dn)
        ? dn.GetString() ?? "—" : "—";

    var created = PrField("creationDate");
    if (DateTimeOffset.TryParse(created, out var dto))
        created = dto.ToString("yyyy-MM-dd HH:mm");

    sb.AppendLine($"## Pull Request #{r.PrId} — {title}");
    sb.AppendLine();
    sb.AppendLine("| Field | Value |");
    sb.AppendLine("|-------|-------|");
    sb.AppendLine($"| State      | {status} |");
    sb.AppendLine($"| Branches   | `{source}` → `{target}` |");
    sb.AppendLine($"| Created By | {createdBy} |");
    sb.AppendLine($"| Created    | {created} |");
    sb.AppendLine();
    sb.AppendLine("### Description");
    sb.AppendLine();
    sb.AppendLine(desc == "—" ? "_No description._" : desc);

    return sb.ToString();
}

string RenderPrsMarkdown(PrsResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine("# Pull Requests");
    sb.AppendLine();
    sb.AppendLine($"**Count:** {r.Count}");
    sb.AppendLine();

    if (r.Count == 0)
    {
        sb.AppendLine("_No PRs found._");
        return sb.ToString();
    }

    sb.AppendLine("| ID | State | Source | Target | Title |");
    sb.AppendLine("|----|-------|--------|--------|-------|");

    foreach (var pr in r.Prs)
    {
        var id = pr.TryGetProperty("pullRequestId", out var idEl) ? idEl.GetInt32().ToString() : "?";
        var state = pr.TryGetProperty("status", out var st) ? st.GetString() ?? "—" : "—";
        var source = pr.TryGetProperty("sourceRefName", out var src) ? StripRefsHeads(src.GetString() ?? "—") : "—";
        var target = pr.TryGetProperty("targetRefName", out var tgt) ? StripRefsHeads(tgt.GetString() ?? "—") : "—";
        var title = pr.TryGetProperty("title", out var ttl) ? ttl.GetString() ?? "—" : "—";
        sb.AppendLine($"| {id} | {state} | `{source}` | `{target}` | {title} |");
    }

    return sb.ToString();
}

string RenderRefsMarkdown(RefsResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine("# Git Refs");
    sb.AppendLine();
    sb.AppendLine($"**Count:** {r.Count}");
    sb.AppendLine();

    if (r.Count == 0)
    {
        sb.AppendLine("_No refs found._");
        return sb.ToString();
    }

    sb.AppendLine("| Name | Object ID |");
    sb.AppendLine("|------|-----------|");

    foreach (var rf in r.Refs)
    {
        var name = rf.TryGetProperty("name", out var n) ? StripRefsHeads(n.GetString() ?? "?") : "?";
        var oid = rf.TryGetProperty("objectId", out var o) ? o.GetString() ?? "?" : "?";
        sb.AppendLine($"| {name} | `{oid}` |");
    }

    return sb.ToString();
}

string RenderRepositoryMarkdown(RepositoryResult r)
{
    var repo = r.Repository;
    var name = repo.TryGetProperty("name", out var n) ? n.GetString() ?? "—" : "—";
    var id = repo.TryGetProperty("id", out var i) ? i.GetString() ?? "—" : "—";
    var project = repo.TryGetProperty("project", out var p) && p.TryGetProperty("name", out var pn) ? pn.GetString() ?? "—" : "—";
    var defaultBranch = repo.TryGetProperty("defaultBranch", out var db) ? StripRefsHeads(db.GetString() ?? "—") : "—";

    var sb = new StringBuilder();
    sb.AppendLine($"## Repository — {name}");
    sb.AppendLine();
    sb.AppendLine("| Field | Value |");
    sb.AppendLine("|-------|-------|");
    sb.AppendLine($"| ID | `{id}` |");
    sb.AppendLine($"| Project | {project} |");
    sb.AppendLine($"| Default Branch | `{defaultBranch}` |");
    return sb.ToString();
}

string RenderPipelineRunMarkdown(PipelineRunResult r)
{
    var sb  = new StringBuilder();
    var run = r.Run;

    string RunField(string name)
        => run.TryGetProperty(name, out var v) ? ExtractString(v) : "—";

    var state    = RunField("state");
    var result   = RunField("result");
    var created  = RunField("createdDate");
    var finished = RunField("finishedDate");

    if (DateTimeOffset.TryParse(created,  out var c)) created  = c.ToString("yyyy-MM-dd HH:mm");
    if (DateTimeOffset.TryParse(finished, out var f)) finished = f.ToString("yyyy-MM-dd HH:mm");

    var branch = "—";
    if (run.TryGetProperty("resources", out var res) &&
        res.TryGetProperty("repositories", out var repos) &&
        repos.TryGetProperty("self", out var self) &&
        self.TryGetProperty("refName", out var refName))
        branch = StripRefsHeads(refName.GetString() ?? "—");

    sb.AppendLine($"## Pipeline {r.PipelineId} — Run #{r.RunId}");
    sb.AppendLine();
    sb.AppendLine("| Field    | Value |");
    sb.AppendLine("|----------|-------|");
    sb.AppendLine($"| State    | {state} |");
    sb.AppendLine($"| Result   | {(result == "—" ? "_(in progress)_" : result)} |");
    sb.AppendLine($"| Branch   | {branch} |");
    sb.AppendLine($"| Created  | {created} |");
    sb.AppendLine($"| Finished | {(finished == "—" ? "_(running)_" : finished)} |");

    return sb.ToString();
}

string RenderBuildRunMarkdown(BuildRunResult r)
{
    var run = r.Run;
    string Field(string name) => run.TryGetProperty(name, out var v) ? ExtractString(v) : "—";

    var pipelineName = run.TryGetProperty("definition", out var def) && def.TryGetProperty("name", out var dn)
        ? dn.GetString() ?? "—"
        : "—";
    var branch = run.TryGetProperty("sourceBranch", out var sourceBranch)
        ? StripRefsHeads(sourceBranch.GetString() ?? "—")
        : "—";

    var sb = new StringBuilder();
    sb.AppendLine($"## Build Run #{r.RunId}");
    sb.AppendLine();
    sb.AppendLine("| Field | Value |");
    sb.AppendLine("|-------|-------|");
    sb.AppendLine($"| Pipeline | {pipelineName} |");
    sb.AppendLine($"| Status | {Field("status")} |");
    sb.AppendLine($"| Result | {Field("result")} |");
    sb.AppendLine($"| Branch | `{branch}` |");
    return sb.ToString();
}

string RenderBuildRunsMarkdown(BuildRunsResult r)
{
    if (r.Count == 0)
        return "# Build Runs\n\n_No runs found._\n";

    return RenderBuildRunMarkdown(new BuildRunResult(
        r.Runs[0].GetProperty("id").GetInt32(),
        r.Runs[0]));
}

string RenderPipelinesMarkdown(PipelinesResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine("# Pipelines");
    sb.AppendLine();
    sb.AppendLine($"**Count:** {r.Count}");
    sb.AppendLine();

    if (r.Count == 0)
    {
        sb.AppendLine("_No pipelines found._");
        return sb.ToString();
    }

    sb.AppendLine("| # | ID | Name | Folder |");
    sb.AppendLine("|---|----|------|--------|");

    int i = 1;
    foreach (var p in r.Pipelines)
    {
        var id     = p.TryGetProperty("id",     out var idEl)     ? idEl.GetInt32().ToString()      : "?";
        var name   = p.TryGetProperty("name",   out var nameEl)   ? nameEl.GetString()   ?? "—"     : "—";
        var folder = p.TryGetProperty("folder", out var folderEl) ? folderEl.GetString() ?? "\\"    : "\\";
        sb.AppendLine($"| {i++} | {id} | {name} | {folder} |");
    }

    return sb.ToString();
}

string RenderWorkflowMarkdown(WorkflowResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine("# Workflow Started");
    sb.AppendLine();
    sb.AppendLine("| | |");
    sb.AppendLine("|-|---|");
    sb.AppendLine($"| Work Item | #{r.WorkItemId} |");
    sb.AppendLine($"| Branch    | `{r.Branch}` |");
    sb.AppendLine($"| Draft PR  | #{r.PrId} |");
    sb.AppendLine($"| Issues    | {r.IssueCount} SonarQube issue(s) in description |");
    sb.AppendLine();
    sb.AppendLine("Work item is **Active**. Branch and draft PR are ready.");
    return sb.ToString();
}

// Extracts a display string from a JsonElement: objects yield displayName, primitives yield their string value.
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

string V(string url) => url.Contains('?') ? $"{url}&api-version=7.1" : $"{url}?api-version=7.1";

string NormalizeRef(string branch)
    => branch.StartsWith("refs/", StringComparison.OrdinalIgnoreCase) ? branch : $"refs/heads/{branch}";

string StripRefsHeads(string refName)
    => refName.StartsWith("refs/heads/", StringComparison.OrdinalIgnoreCase) ? refName["refs/heads/".Length..] : refName;

HttpClient CreateHttpClient()
{
    var adoUrl  = Environment.GetEnvironmentVariable("ADO_URL")?.TrimEnd('/')
        ?? throw new InvalidOperationException("ADO_URL environment variable is not set.");
    var project = Environment.GetEnvironmentVariable("ADO_PROJECT")
        ?? throw new InvalidOperationException("ADO_PROJECT environment variable is not set.");
    var pat = Environment.GetEnvironmentVariable("ADO_PAT")
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
        return 1;
    }
    return 0;
}

async Task<string> GetDefaultBranch(HttpClient http, string repo)
{
    var repoInfo = await GetRepository(http, repo);
    if (repoInfo is null) return "refs/heads/main";
    var root = repoInfo.Value;
    if (root.TryGetProperty("defaultBranch", out var db))
        return StripRefsHeads(db.GetString() ?? "refs/heads/main");
    return "refs/heads/main";
}

async Task<JsonElement?> GetWorkItem(HttpClient http, int id, bool expandRelations = false)
{
    var url = expandRelations ? V($"wit/workitems/{id}?$expand=relations") : V($"wit/workitems/{id}");
    var resp = await http.GetAsync(url);
    if (await ExitOnError(resp) != 0) return null;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    return doc.RootElement.Clone();
}

async Task<JsonElement?> GetRepository(HttpClient http, string repo)
{
    var resp = await http.GetAsync(V($"git/repositories/{Uri.EscapeDataString(repo)}"));
    if (await ExitOnError(resp) != 0) return null;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    return doc.RootElement.Clone();
}

JsonObject BuildRelationAddOperation(string rel, string url, JsonObject? attributes = null)
{
    var value = new JsonObject
    {
        ["rel"] = rel,
        ["url"] = url
    };
    if (attributes is not null)
        value["attributes"] = attributes;

    return new JsonObject
    {
        ["op"] = "add",
        ["path"] = "/relations/-",
        ["value"] = value
    };
}

string BranchArtifactUrl(string projectId, string repoId, string branch)
    => $"vstfs:///Git/Ref/{projectId}%2F{repoId}%2FGB{Uri.EscapeDataString(branch)}";

string PullRequestArtifactUrl(string projectId, string repoId, int prId)
    => $"vstfs:///Git/PullRequestId/{projectId}%2F{repoId}%2F{prId}";

string WorkItemUrl(int id)
{
    var adoUrl = Environment.GetEnvironmentVariable("ADO_URL")?.TrimEnd('/')
        ?? throw new InvalidOperationException("ADO_URL environment variable is not set.");
    var project = Environment.GetEnvironmentVariable("ADO_PROJECT")
        ?? throw new InvalidOperationException("ADO_PROJECT environment variable is not set.");
    return $"{adoUrl}/{Uri.EscapeDataString(project)}/_apis/wit/workItems/{id}";
}

// ── result types ──────────────────────────────────────────────────────────────

record WorkItemResult(int Id, JsonElement Fields, JsonElement? Relations);
record WorkItemsResult(int Count, List<JsonElement> Items);
record PrResult(int PrId, JsonElement Pr);
record PrsResult(int Count, List<JsonElement> Prs);
record RefsResult(int Count, List<JsonElement> Refs);
record RepositoryResult(JsonElement Repository);
record PipelineRunResult(int PipelineId, int RunId, JsonElement Run);
record BuildRunResult(int RunId, JsonElement Run);
record BuildRunsResult(int Count, List<JsonElement> Runs);
record PipelinesResult(int Count, List<JsonElement> Pipelines);
record WorkflowResult(int WorkItemId, string Branch, int PrId, int IssueCount);
