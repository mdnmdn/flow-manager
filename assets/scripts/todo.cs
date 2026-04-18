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

var formatOpt = new Option<string>("--format", () => "markdown", "Output format: json, yaml, markdown");

var scriptDir = Path.GetDirectoryName(Environment.ProcessPath ?? string.Empty)
    ?? Path.GetDirectoryName(AppContext.BaseDirectory)
    ?? ".";
var adoScript = Path.Combine(scriptDir, "ado.cs");

var wiIdOpt = new Option<int>("--wi-id", "Parent work item ID") { IsRequired = true };

var showAllOpt = new Option<bool>("--all", "Include closed items");
var showDetailOpt = new Option<bool>("--detail", "Include descriptions");
var showCmd = new Command("show", "Show child task todos for a work item")
{
    wiIdOpt, showAllOpt, showDetailOpt, formatOpt
};
showCmd.SetHandler(async (int wiId, bool all, bool detail, string format) =>
{
    var snapshot = await LoadSnapshot(wiId, includeClosed: all);
    Render(format, snapshot with { ShowDetail = detail });
}, wiIdOpt, showAllOpt, showDetailOpt, formatOpt);

var newTitleOpt = new Option<string>("--title", "Task title") { IsRequired = true };
var newDescriptionOpt = new Option<string?>("--description", () => null, "Task description");
var newAssignedToOpt = new Option<string?>("--assigned-to", () => null, "Assigned-to");
var newPickOpt = new Option<bool>("--pick", "Immediately set the todo Active");
var newCmd = new Command("new", "Create a child Task todo")
{
    wiIdOpt, newTitleOpt, newDescriptionOpt, newAssignedToOpt, newPickOpt, formatOpt
};
newCmd.SetHandler(async (int wiId, string title, string? description, string? assignedTo, bool pick, string format) =>
{
    var created = await RunAdoJson("wi", "create", "--type", "Task", "--title", title, "--format", "json",
        description is null ? null : "--description",
        description,
        assignedTo is null ? null : "--assigned-to",
        assignedTo);

    var taskId = created.GetProperty("id").GetInt32();
    await RunAdoJson("wi", "add-child", "--parent-id", wiId.ToString(), "--child-id", taskId.ToString(), "--format", "json");

    if (pick)
    {
        await RunAdoJson("wi", "update", "--id", taskId.ToString(), "--state", "Active", "--format", "json");
    }

    var snapshot = await LoadSnapshot(wiId, includeClosed: true);
    var todo = GetTodoById(snapshot, taskId);
    Render(format, new TodoActionResult("added", todo, false, snapshot with { ShowDetail = true }));
}, wiIdOpt, newTitleOpt, newDescriptionOpt, newAssignedToOpt, newPickOpt, formatOpt);

var refArg = new Argument<string>("ref", "Task id or case-insensitive title fragment");

var pickCmd = new Command("pick", "Set a todo Active")
{
    wiIdOpt, refArg, formatOpt
};
pickCmd.SetHandler(async (int wiId, string @ref, string format) =>
{
    await ChangeTodoState(wiId, @ref, "Active", "picked", format);
}, wiIdOpt, refArg, formatOpt);

var completeCmd = new Command("complete", "Set a todo Closed")
{
    wiIdOpt, refArg, formatOpt
};
completeCmd.SetHandler(async (int wiId, string @ref, string format) =>
{
    await ChangeTodoState(wiId, @ref, "Closed", "completed", format);
}, wiIdOpt, refArg, formatOpt);

var reopenCmd = new Command("reopen", "Set a todo back to New")
{
    wiIdOpt, refArg, formatOpt
};
reopenCmd.SetHandler(async (int wiId, string @ref, string format) =>
{
    await ChangeTodoState(wiId, @ref, "New", "reopened", format);
}, wiIdOpt, refArg, formatOpt);

var updateTitleOpt = new Option<string?>("--title", () => null, "New title");
var updateDescriptionOpt = new Option<string?>("--description", () => null, "New description");
var updateAssignedOpt = new Option<string?>("--assigned-to", () => null, "New assigned-to");
var updateStateOpt = new Option<string?>("--state", () => null, "State: New, Active, Closed");
var updateCmd = new Command("update", "Update a todo")
{
    wiIdOpt, refArg, updateTitleOpt, updateDescriptionOpt, updateAssignedOpt, updateStateOpt, formatOpt
};
updateCmd.SetHandler(async (int wiId, string @ref, string? title, string? description, string? assignedTo, string? state, string format) =>
{
    var snapshot = await LoadSnapshot(wiId, includeClosed: true);
    var resolved = ResolveTodo(snapshot, @ref);

    var args = new List<string> { "wi", "update", "--id", resolved.Id.ToString(), "--format", "json" };
    if (!string.IsNullOrWhiteSpace(title))
    {
        args.Add("--title");
        args.Add(title);
    }
    if (!string.IsNullOrWhiteSpace(description))
    {
        args.Add("--description");
        args.Add(description);
    }
    if (!string.IsNullOrWhiteSpace(assignedTo))
    {
        args.Add("--assigned-to");
        args.Add(assignedTo);
    }
    if (!string.IsNullOrWhiteSpace(state))
    {
        args.Add("--state");
        args.Add(state);
    }

    await RunAdoJson(args.ToArray());

    var refreshed = await LoadSnapshot(wiId, includeClosed: true);
    var todo = ResolveTodo(refreshed, resolved.Id.ToString());
    Render(format, new TodoActionResult("updated", todo, false, refreshed with { ShowDetail = true }));
}, wiIdOpt, refArg, updateTitleOpt, updateDescriptionOpt, updateAssignedOpt, updateStateOpt, formatOpt);

var nextPickOpt = new Option<bool>("--pick", "Immediately set the next todo Active");
var nextCmd = new Command("next", "Show the next New todo")
{
    wiIdOpt, nextPickOpt, formatOpt
};
nextCmd.SetHandler(async (int wiId, bool pick, string format) =>
{
    var snapshot = await LoadSnapshot(wiId, includeClosed: true);
    var next = snapshot.New.OrderBy(x => x.Id).FirstOrDefault();

    if (next is null)
    {
        Render(format, new TodoActionResult("next", null, true, snapshot));
        return;
    }

    if (pick)
    {
        await RunAdoJson("wi", "update", "--id", next.Id.ToString(), "--state", "Active", "--format", "json");
        snapshot = await LoadSnapshot(wiId, includeClosed: true);
        next = ResolveTodo(snapshot, next.Id.ToString());
    }

    Render(format, new TodoActionResult("next", next, false, snapshot with { ShowDetail = true }));
}, wiIdOpt, nextPickOpt, formatOpt);

var rootCmd = new RootCommand("Todo workflow commands over ADO work items")
{
    showCmd,
    newCmd,
    pickCmd,
    completeCmd,
    reopenCmd,
    updateCmd,
    nextCmd
};

return await rootCmd.InvokeAsync(args);

async Task ChangeTodoState(int wiId, string reference, string targetState, string action, string format)
{
    var snapshot = await LoadSnapshot(wiId, includeClosed: true);
    var resolved = ResolveTodo(snapshot, reference);
    var already = string.Equals(resolved.State, targetState, StringComparison.OrdinalIgnoreCase);

    if (!already)
    {
        await RunAdoJson("wi", "update", "--id", resolved.Id.ToString(), "--state", targetState, "--format", "json");
        snapshot = await LoadSnapshot(wiId, includeClosed: true);
        resolved = ResolveTodo(snapshot, resolved.Id.ToString());
    }

    Render(format, new TodoActionResult(action, resolved, already, snapshot with { ShowDetail = true }));
}

async Task<TodoSnapshot> LoadSnapshot(int wiId, bool includeClosed)
{
    var parentTask = RunAdoJson("wi", "get", "--id", wiId.ToString(), "--format", "json");
    var childrenTask = RunAdoJson("wi", "get-children", "--id", wiId.ToString(), "--format", "json");
    await Task.WhenAll(parentTask, childrenTask);

    var parent = await parentTask;
    var childrenRoot = await childrenTask;

    var parentTitle = ExtractField(parent.GetProperty("fields"), "System.Title");
    var items = childrenRoot.TryGetProperty("items", out var rawItems)
        ? rawItems.EnumerateArray().Select(ToTodoItem).Where(x => x.Type == "Task").ToList()
        : new List<TodoItem>();

    var active = items.Where(x => x.State == "Active").OrderBy(x => x.Id).ToList();
    var @new = items.Where(x => x.State == "New").OrderBy(x => x.Id).ToList();
    var closed = items.Where(x => x.State == "Closed").OrderBy(x => x.Id).ToList();

    if (!includeClosed)
        closed = new List<TodoItem>();

    return new TodoSnapshot(wiId, parentTitle, active, @new, closed, false);
}

TodoItem ResolveTodo(TodoSnapshot snapshot, string reference)
{
    var all = snapshot.Active.Concat(snapshot.New).Concat(snapshot.Closed).OrderBy(x => x.Id).ToList();
    if (int.TryParse(reference, out var id))
    {
        var direct = all.FirstOrDefault(x => x.Id == id);
        if (direct is null)
        {
            Console.Error.WriteLine($"No todo #{id} found under WI #{snapshot.ParentId}.");
            Environment.Exit(1);
            throw new InvalidOperationException();
        }
        return direct;
    }

    var matches = all
        .Where(x => x.Title.Contains(reference, StringComparison.OrdinalIgnoreCase))
        .ToList();

    if (matches.Count == 1)
        return matches[0];

    if (matches.Count == 0)
    {
        Console.Error.WriteLine($"No todo matching '{reference}' found under WI #{snapshot.ParentId}.");
        Console.Error.WriteLine("Run `todo.cs show` or `fm todo show` to inspect available items.");
        Environment.Exit(1);
        throw new InvalidOperationException();
    }

    Console.Error.WriteLine($"Multiple todos match '{reference}':");
    foreach (var match in matches)
        Console.Error.WriteLine($"  #{match.Id}: {match.Title}");
    Environment.Exit(1);
    throw new InvalidOperationException();
}

TodoItem GetTodoById(TodoSnapshot snapshot, int id)
    => ResolveTodo(snapshot, id.ToString());

TodoItem ToTodoItem(JsonElement item)
{
    var fields = item.GetProperty("fields");
    return new TodoItem(
        item.GetProperty("id").GetInt32(),
        ExtractField(fields, "System.Title"),
        ExtractField(fields, "System.State"),
        ExtractField(fields, "System.WorkItemType"),
        ExtractField(fields, "System.Description"),
        ExtractField(fields, "System.AssignedTo"));
}

string ExtractField(JsonElement fields, string name)
{
    if (!fields.TryGetProperty(name, out var value))
        return "—";

    return value.ValueKind switch
    {
        JsonValueKind.Null => "—",
        JsonValueKind.Object => value.TryGetProperty("displayName", out var dn) ? dn.GetString() ?? "—" : "—",
        _ => value.GetString() ?? "—"
    };
}

async Task<JsonElement> RunAdoJson(params string?[] rawArgs)
{
    var args = rawArgs.Where(x => !string.IsNullOrWhiteSpace(x)).Select(x => x!).ToList();

    var psi = new ProcessStartInfo("dotnet")
    {
        RedirectStandardOutput = true,
        RedirectStandardError = true,
        UseShellExecute = false
    };

    psi.ArgumentList.Add("run");
    psi.ArgumentList.Add(adoScript);
    psi.ArgumentList.Add("--");
    foreach (var arg in args)
        psi.ArgumentList.Add(arg);

    using var proc = Process.Start(psi) ?? throw new InvalidOperationException("Failed to start ado.cs.");
    var stdout = await proc.StandardOutput.ReadToEndAsync();
    var stderr = await proc.StandardError.ReadToEndAsync();
    await proc.WaitForExitAsync();

    if (proc.ExitCode != 0)
    {
        if (!string.IsNullOrWhiteSpace(stderr))
            Console.Error.WriteLine(stderr.Trim());
        Environment.Exit(proc.ExitCode);
        throw new InvalidOperationException("ado.cs failed.");
    }

    using var doc = JsonDocument.Parse(stdout);
    return doc.RootElement.Clone();
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
    TodoSnapshot x => RenderSnapshotMarkdown(x),
    TodoActionResult x => RenderActionMarkdown(x),
    _ => throw new NotSupportedException($"No markdown renderer for {data.GetType().Name}")
};

object? ToYamlObject(object? value) => value switch
{
    TodoSnapshot x => new Dictionary<string, object?>
    {
        ["parentId"] = x.ParentId,
        ["parentTitle"] = x.ParentTitle,
        ["active"] = x.Active.Select(ToYamlObject).ToList(),
        ["new"] = x.New.Select(ToYamlObject).ToList(),
        ["closed"] = x.Closed.Select(ToYamlObject).ToList()
    },
    TodoActionResult x => new Dictionary<string, object?>
    {
        ["action"] = x.Action,
        ["idempotent"] = x.Idempotent,
        ["todo"] = ToYamlObject(x.Todo),
        ["snapshot"] = ToYamlObject(x.Snapshot)
    },
    TodoItem x => new Dictionary<string, object?>
    {
        ["id"] = x.Id,
        ["title"] = x.Title,
        ["state"] = x.State,
        ["type"] = x.Type,
        ["description"] = x.Description,
        ["assignedTo"] = x.AssignedTo
    },
    _ => value
};

string RenderActionMarkdown(TodoActionResult result)
{
    var heading = result.Action switch
    {
        "added" => "Todo Added",
        "picked" => "Todo Active",
        "completed" => "Todo Closed",
        "reopened" => "Todo Reopened",
        "updated" => "Todo Updated",
        "next" => "Next Todo",
        _ => "Todo"
    };

    var sb = new StringBuilder();
    if (result.Todo is null)
    {
        sb.AppendLine($"## {heading}");
        sb.AppendLine();
        sb.AppendLine("No open todos found.");
        sb.AppendLine();
        sb.AppendLine(RenderCounts(result.Snapshot));
        return sb.ToString();
    }

    sb.AppendLine($"## {heading} — #{result.Todo.Id}: {result.Todo.Title}");
    sb.AppendLine();

    if (result.Todo.Description != "—")
        sb.AppendLine($"{result.Todo.Description}\n");

    if (result.Action == "updated")
    {
        sb.AppendLine($"Title       {result.Todo.Title}");
        sb.AppendLine($"Description {result.Todo.Description}");
        sb.AppendLine($"State       {result.Todo.State}");
        sb.AppendLine($"Assigned    {result.Todo.AssignedTo}");
        sb.AppendLine();
    }

    sb.AppendLine(RenderCompactList(result.Snapshot, result.Todo.Id, includeClosed: true));
    if (result.Idempotent)
    {
        sb.AppendLine();
        sb.AppendLine("Already in the requested state.");
    }
    return sb.ToString();
}

string RenderSnapshotMarkdown(TodoSnapshot snapshot)
{
    var sb = new StringBuilder();
    sb.AppendLine($"## Todos — #{snapshot.ParentId}: {snapshot.ParentTitle}");
    sb.AppendLine();
    sb.AppendLine(RenderCompactList(snapshot, null, includeClosed: snapshot.Closed.Count > 0));
    return sb.ToString();
}

string RenderCompactList(TodoSnapshot snapshot, int? highlightId, bool includeClosed)
{
    var sb = new StringBuilder();

    void RenderItems(IEnumerable<TodoItem> items, string glyph)
    {
        foreach (var todo in items)
        {
            var suffix = todo.Id == highlightId ? "  <- updated" : string.Empty;
            var stateText = todo.State == "New" ? string.Empty : todo.State;
            var line = $"  {glyph}  #{todo.Id}  {todo.Title}";
            if (!string.IsNullOrWhiteSpace(stateText))
                line = line.PadRight(58) + stateText;
            sb.AppendLine(line + suffix);

            if (snapshot.ShowDetail && todo.Description != "—")
                sb.AppendLine($"             {todo.Description}");
        }
    }

    RenderItems(snapshot.Active, "●");
    RenderItems(snapshot.New, "○");
    if (includeClosed)
        RenderItems(snapshot.Closed, "✓");

    sb.AppendLine();
    sb.AppendLine("  -----------------------------------------");
    sb.AppendLine($"  {RenderCounts(snapshot)}");
    return sb.ToString().TrimEnd();
}

string RenderCounts(TodoSnapshot snapshot)
{
    var done = snapshot.Closed.Count;
    var active = snapshot.Active.Count;
    var open = snapshot.New.Count;
    var total = done + active + open;
    return $"{done} done · {active} active · {open} open · {total} total";
}

record TodoItem(int Id, string Title, string State, string Type, string Description, string AssignedTo);
record TodoSnapshot(int ParentId, string ParentTitle, List<TodoItem> Active, List<TodoItem> New, List<TodoItem> Closed, bool ShowDetail);
record TodoActionResult(string Action, TodoItem? Todo, bool Idempotent, TodoSnapshot Snapshot);
