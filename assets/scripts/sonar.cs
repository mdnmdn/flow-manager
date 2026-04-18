#!/usr/bin/dotnet run
#:package DotNetEnv@3.1.1
#:package System.CommandLine@2.0.0-beta4.22272.1
#:package YamlDotNet@16.3.0
#:property PublishAot=false
#:property PublishTrimmed=false

using System.CommandLine;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
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

// ── issues ────────────────────────────────────────────────────────────────────

var issuesProjectOpt = new Option<string>("--project", "SonarQube project key") { IsRequired = true };
var severityOpt      = new Option<string?>("--severity", () => null, "Comma-separated severities: INFO,MINOR,MAJOR,CRITICAL,BLOCKER");
var maxResultsOpt    = new Option<int>("--max-results", () => 100, "Maximum number of issues to return (1-500)");

var issuesCmd = new Command("issues", "Fetch issues from SonarQube")
    { issuesProjectOpt, severityOpt, maxResultsOpt, formatOpt };

issuesCmd.SetHandler(async (string project, string? severity, int maxResults, string format) =>
{
    maxResults = Math.Clamp(maxResults, 1, 500);
    using var http = CreateHttpClient();

    var severityParam = severity is not null
        ? string.Join(",", severity.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)).ToUpperInvariant()
        : null;

    var allIssues = new List<JsonElement>();
    int pageSize = Math.Min(maxResults, 100);
    int page = 1;

    Console.Error.WriteLine($"Fetching issues for project '{project}'...");

    while (allIssues.Count < maxResults)
    {
        var qs = new StringBuilder(
            $"/api/issues/search?componentKeys={Uri.EscapeDataString(project)}&ps={pageSize}&p={page}&resolved=false");
        if (severityParam is not null)
            qs.Append($"&severities={Uri.EscapeDataString(severityParam)}");

        var resp = await http.GetAsync(qs.ToString());
        if (await ExitOnError(resp) != 0) return;

        using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
        var root  = doc.RootElement;
        int total = root.GetProperty("total").GetInt32();
        var batch = root.GetProperty("issues").EnumerateArray().Select(e => e.Clone()).ToList();

        allIssues.AddRange(batch);
        Console.Error.WriteLine($"  Page {page}: got {batch.Count} issues (total available: {total})");

        if (allIssues.Count >= maxResults || allIssues.Count >= total) break;

        pageSize = Math.Min(maxResults - allIssues.Count, 100);
        page++;
    }

    var trimmed = allIssues.Take(maxResults).ToList();
    Render(format, new IssuesResult(project, trimmed.Count, trimmed));
    Console.Error.WriteLine($"Done. {trimmed.Count} issue(s) returned.");

}, issuesProjectOpt, severityOpt, maxResultsOpt, formatOpt);

// ── project list ──────────────────────────────────────────────────────────────

var searchOpt    = new Option<string?>("--search",    () => null, "Wildcard search on project name/key (e.g. mauto*)");
var favoritesOpt = new Option<bool>  ("--favorites",             "Return only projects marked as favorites");

var projectListCmd = new Command("list", "List SonarQube projects") { searchOpt, favoritesOpt, formatOpt };

projectListCmd.SetHandler(async (string? search, bool favorites, string format) =>
{
    using var http = CreateHttpClient();

    var endpoint = favorites
        ? "/api/favorites/search?ps=500"
        : $"/api/projects/search?ps=500{(search is not null ? $"&q={Uri.EscapeDataString(search)}" : "")}";

    Console.Error.WriteLine($"Fetching project list{(favorites ? " (favorites)" : "")}...");

    var resp = await http.GetAsync(endpoint);
    if (await ExitOnError(resp) != 0) return;

    using var doc = await JsonDocument.ParseAsync(await resp.Content.ReadAsStreamAsync());
    var components = doc.RootElement
        .GetProperty("components")
        .EnumerateArray()
        .Select(e => e.Clone())
        .ToList();

    if (favorites && search is not null)
    {
        var pattern = "^" + System.Text.RegularExpressions.Regex.Escape(search)
            .Replace(@"\*", ".*").Replace(@"\?", ".") + "$";
        var rx = new System.Text.RegularExpressions.Regex(
            pattern, System.Text.RegularExpressions.RegexOptions.IgnoreCase);

        components = components
            .Where(e =>
                (e.TryGetProperty("key",  out var k) && rx.IsMatch(k.GetString() ?? "")) ||
                (e.TryGetProperty("name", out var n) && rx.IsMatch(n.GetString() ?? "")))
            .ToList();
    }

    Render(format, new ProjectsResult(components.Count, components));
    Console.Error.WriteLine($"Done. {components.Count} project(s) returned.");

}, searchOpt, favoritesOpt, formatOpt);

var projectCmd = new Command("project", "Manage SonarQube projects") { projectListCmd };

// ── root ──────────────────────────────────────────────────────────────────────

var rootCmd = new RootCommand("SonarQube CLI utilities") { issuesCmd, projectCmd };
return await rootCmd.InvokeAsync(args);

// ── local functions ───────────────────────────────────────────────────────────

void Render(string format, object data)
{
    var output = format.ToLowerInvariant() switch
    {
        "yaml"     => yaml.Serialize(ToYamlObject(data)),
        "markdown" => data switch
        {
            IssuesResult   r => RenderIssuesMarkdown(r),
            ProjectsResult r => RenderProjectsMarkdown(r),
            _                => throw new NotSupportedException($"No markdown renderer for {data.GetType().Name}")
        },
        _ => JsonSerializer.Serialize(data, jsonOpts)
    };
    Console.WriteLine(output);
}

// Recursively converts JsonElement values to plain .NET objects so YamlDotNet can serialize them.
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
    IssuesResult   r => new Dictionary<string, object?> {
        ["project"] = r.Project,
        ["fetched"] = r.Fetched,
        ["issues"]  = r.Issues.Select(e => ToYamlObject(e)).ToList()
    },
    ProjectsResult r => new Dictionary<string, object?> {
        ["fetched"]  = r.Fetched,
        ["projects"] = r.Projects.Select(e => ToYamlObject(e)).ToList()
    },
    _ => value
};

string RenderIssuesMarkdown(IssuesResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine($"# SonarQube Issues — `{r.Project}`");
    sb.AppendLine();
    sb.AppendLine($"**Fetched:** {r.Fetched}");
    sb.AppendLine();

    if (r.Issues.Count == 0)
    {
        sb.AppendLine("_No issues found._");
        return sb.ToString();
    }

    int i = 1;
    foreach (var issue in r.Issues)
    {
        var key       = issue.TryGetProperty("key",       out var k) ? k.GetString() : "";
        var severity  = issue.TryGetProperty("severity",  out var s) ? s.GetString() : "";
        var type      = issue.TryGetProperty("type",      out var t) ? t.GetString() : "";
        var message   = issue.TryGetProperty("message",   out var m) ? m.GetString() : "";
        var component = issue.TryGetProperty("component", out var c) ? c.GetString() : "";

        var location = "";
        if (issue.TryGetProperty("textRange", out var tr))
        {
            var startLine   = tr.TryGetProperty("startLine",   out var sl) ? sl.GetInt32().ToString() : "?";
            var endLine     = tr.TryGetProperty("endLine",     out var el) ? el.GetInt32().ToString() : "?";
            var startOffset = tr.TryGetProperty("startOffset", out var so) ? so.GetInt32().ToString() : "?";
            var endOffset   = tr.TryGetProperty("endOffset",   out var eo) ? eo.GetInt32().ToString() : "?";
            location = startLine == endLine
                ? $"L{startLine} [{startOffset}-{endOffset}]"
                : $"L{startLine}-{endLine} [{startOffset}-{endOffset}]";
        }

        var componentLine = location != "" ? $"{component} {location}" : component;
        sb.AppendLine($"""
            ### {i++}. {severity} — {type} `{key}`

            {message}

            {componentLine}

            ---

            """);
    }

    return sb.ToString();
}

string RenderProjectsMarkdown(ProjectsResult r)
{
    var sb = new StringBuilder();
    sb.AppendLine("# SonarQube Projects");
    sb.AppendLine();
    sb.AppendLine($"**Fetched:** {r.Fetched}");
    sb.AppendLine();

    if (r.Projects.Count == 0)
    {
        sb.AppendLine("_No projects found._");
        return sb.ToString();
    }

    sb.AppendLine("| # | Key | Name | Visibility | Last Analysis |");
    sb.AppendLine("|---|-----|------|------------|---------------|");

    int i = 1;
    foreach (var p in r.Projects)
    {
        var key        = p.TryGetProperty("key",              out var k) ? k.GetString() : "";
        var name       = p.TryGetProperty("name",             out var n) ? n.GetString() : "";
        var visibility = p.TryGetProperty("visibility",       out var v) ? v.GetString() : "";
        var lastDate   = p.TryGetProperty("lastAnalysisDate", out var d) ? d.GetString() : "—";

        if (lastDate is not null && DateTimeOffset.TryParse(lastDate, out var dto))
            lastDate = dto.ToString("yyyy-MM-dd");

        sb.AppendLine($"| {i++} | `{key}` | {name} | {visibility} | {lastDate} |");
    }

    return sb.ToString();
}

HttpClient CreateHttpClient()
{
    var baseUrl = Environment.GetEnvironmentVariable("SONAR_URL")?.TrimEnd('/')
        ?? throw new InvalidOperationException("SONAR_URL environment variable is not set.");
    var token = Environment.GetEnvironmentVariable("SONAR_TOKEN")
        ?? throw new InvalidOperationException("SONAR_TOKEN environment variable is not set.");

    var http = new HttpClient { BaseAddress = new Uri(baseUrl) };
    var encoded = Convert.ToBase64String(Encoding.UTF8.GetBytes($"{token}:"));
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

// ── result types ──────────────────────────────────────────────────────────────

record IssuesResult(string Project, int Fetched, List<JsonElement> Issues);
record ProjectsResult(int Fetched, List<JsonElement> Projects);
