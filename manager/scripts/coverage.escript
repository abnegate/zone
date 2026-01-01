#!/usr/bin/env escript
%% -*- erlang -*-
%%! -pa build/dev/erlang/*/ebin

-mode(compile).

main(Args) ->
    io:format("~n=== Gleam Code Coverage Analysis ===~n~n"),

    %% Parse arguments
    Verbose = lists:member("--verbose", Args) orelse lists:member("-v", Args),

    %% Start cover
    cover:start(),

    %% Find all application modules (exclude dependencies and test modules)
    EbinDir = "build/dev/erlang/manager/ebin",
    {ok, BeamFiles} = file:list_dir(EbinDir),

    Modules = [list_to_atom(filename:basename(F, ".beam"))
               || F <- BeamFiles,
                  filename:extension(F) == ".beam",
                  not is_test_module(F),
                  not is_dependency(F)],

    io:format("Instrumenting ~p modules for coverage...~n", [length(Modules)]),

    %% Compile modules for coverage
    lists:foreach(fun(Mod) ->
        BeamPath = filename:join(EbinDir, atom_to_list(Mod) ++ ".beam"),
        case cover:compile_beam(BeamPath) of
            {ok, _} ->
                if Verbose -> io:format("  Compiled: ~p~n", [Mod]); true -> ok end;
            {error, Reason} ->
                io:format("  Warning: Could not compile ~p: ~p~n", [Mod, Reason])
        end
    end, Modules),

    io:format("~nRunning tests...~n~n"),

    %% Run gleeunit tests
    TestResult = os:cmd("gleam test 2>&1"),
    io:format("~s~n", [TestResult]),

    %% Analyze coverage
    io:format("~n=== Coverage Results ===~n~n"),

    %% Create coverage directory
    CoverageDir = "coverage",
    filelib:ensure_dir(CoverageDir ++ "/"),

    %% Analyze each module and collect stats
    {TotalCovered, TotalLines, ModuleStats} = analyze_modules(Modules, CoverageDir, Verbose),

    %% Calculate overall coverage
    CoveragePercent = case TotalLines of
        0 -> 0.0;
        _ -> (TotalCovered / TotalLines) * 100
    end,

    %% Print summary
    io:format("~n=== Coverage Summary ===~n~n"),
    io:format("Total Lines:    ~p~n", [TotalLines]),
    io:format("Covered Lines:  ~p~n", [TotalCovered]),
    io:format("Coverage:       ~.1f%~n~n", [CoveragePercent]),

    %% Print per-module breakdown
    io:format("Per-module coverage:~n"),
    io:format("~-50s ~10s ~10s ~10s~n", ["Module", "Lines", "Covered", "Coverage"]),
    io:format("~s~n", [string:copies("-", 82)]),

    SortedStats = lists:reverse(lists:keysort(4, ModuleStats)),
    lists:foreach(fun({Mod, Lines, Covered, Pct}) ->
        ModStr = truncate_module_name(atom_to_list(Mod), 48),
        Status = if Pct >= 80 -> ""; Pct >= 50 -> " (!)"; true -> " (!!)" end,
        io:format("~-50s ~10p ~10p ~8.1f%~s~n", [ModStr, Lines, Covered, Pct, Status])
    end, SortedStats),

    io:format("~s~n", [string:copies("-", 82)]),
    io:format("~-50s ~10p ~10p ~8.1f%~n~n", ["TOTAL", TotalLines, TotalCovered, CoveragePercent]),

    %% Generate HTML report
    generate_html_report(CoverageDir, ModuleStats, CoveragePercent),

    %% Export coverage data
    cover:export(filename:join(CoverageDir, "coverage.coverdata")),

    io:format("Coverage report generated in: ~s/~n", [CoverageDir]),
    io:format("  - index.html (summary)~n"),
    io:format("  - *.html (per-module details)~n"),
    io:format("  - coverage.coverdata (raw data)~n~n"),

    %% Check threshold
    Threshold = 80.0,
    if
        CoveragePercent >= Threshold ->
            io:format("✓ Coverage meets threshold (~.1f% >= ~.1f%)~n~n", [CoveragePercent, Threshold]),
            halt(0);
        true ->
            io:format("✗ Coverage below threshold (~.1f% < ~.1f%)~n~n", [CoveragePercent, Threshold]),
            halt(1)
    end.

is_test_module(Filename) ->
    lists:suffix("_test.beam", Filename).

is_dependency(_Filename) ->
    %% All modules in manager/ebin are our code
    false.

truncate_module_name(Name, MaxLen) ->
    case length(Name) > MaxLen of
        true -> string:slice(Name, 0, MaxLen - 3) ++ "...";
        false -> Name
    end.

analyze_modules(Modules, CoverageDir, Verbose) ->
    lists:foldl(fun(Mod, {AccCovered, AccTotal, AccStats}) ->
        case cover:analyse(Mod, coverage, line) of
            {ok, {_, {Covered, NotCovered}}} ->
                Total = Covered + NotCovered,
                Pct = case Total of
                    0 -> 100.0;
                    _ -> (Covered / Total) * 100
                end,

                %% Generate per-module HTML
                HtmlFile = filename:join(CoverageDir, atom_to_list(Mod) ++ ".html"),
                cover:analyse_to_file(Mod, HtmlFile, [html]),

                if Verbose ->
                    io:format("  ~p: ~.1f% (~p/~p lines)~n", [Mod, Pct, Covered, Total]);
                true -> ok end,

                {AccCovered + Covered, AccTotal + Total, [{Mod, Total, Covered, Pct} | AccStats]};
            {error, _Reason} ->
                {AccCovered, AccTotal, AccStats}
        end
    end, {0, 0, []}, Modules).

generate_html_report(CoverageDir, ModuleStats, TotalCoverage) ->
    IndexFile = filename:join(CoverageDir, "index.html"),

    SortedStats = lists:reverse(lists:keysort(4, ModuleStats)),

    ModuleRows = lists:map(fun({Mod, Lines, Covered, Pct}) ->
        ModName = atom_to_list(Mod),
        StatusClass = if Pct >= 80 -> "high"; Pct >= 50 -> "medium"; true -> "low" end,
        io_lib:format(
            "<tr class=\"~s\">"
            "<td><a href=\"~s.html\">~s</a></td>"
            "<td>~p</td>"
            "<td>~p</td>"
            "<td>~.1f%</td>"
            "</tr>~n",
            [StatusClass, ModName, ModName, Lines, Covered, Pct])
    end, SortedStats),

    StatusClass = if TotalCoverage >= 80 -> "high"; TotalCoverage >= 50 -> "medium"; true -> "low" end,

    Html = io_lib:format(
"<!DOCTYPE html>
<html>
<head>
    <title>Gleam Code Coverage Report</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; margin: 40px; background: #f5f5f5; }
        .container { max-width: 1200px; margin: 0 auto; background: white; padding: 30px; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }
        h1 { color: #333; border-bottom: 2px solid #5850ec; padding-bottom: 10px; }
        .summary { display: flex; gap: 20px; margin: 20px 0; }
        .stat { background: #f8f9fa; padding: 20px; border-radius: 8px; text-align: center; flex: 1; }
        .stat-value { font-size: 2em; font-weight: bold; }
        .stat-label { color: #666; margin-top: 5px; }
        .high .stat-value { color: #22c55e; }
        .medium .stat-value { color: #f59e0b; }
        .low .stat-value { color: #ef4444; }
        table { width: 100%; border-collapse: collapse; margin-top: 20px; }
        th, td { padding: 12px; text-align: left; border-bottom: 1px solid #eee; }
        th { background: #f8f9fa; font-weight: 600; }
        tr:hover { background: #f8f9fa; }
        tr.high td:last-child { color: #22c55e; }
        tr.medium td:last-child { color: #f59e0b; }
        tr.low td:last-child { color: #ef4444; }
        a { color: #5850ec; text-decoration: none; }
        a:hover { text-decoration: underline; }
        .threshold { margin-top: 20px; padding: 15px; border-radius: 8px; }
        .threshold.pass { background: #dcfce7; color: #166534; }
        .threshold.fail { background: #fee2e2; color: #991b1b; }
    </style>
</head>
<body>
    <div class=\"container\">
        <h1>Gleam Code Coverage Report</h1>

        <div class=\"summary\">
            <div class=\"stat ~s\">
                <div class=\"stat-value\">~.1f%</div>
                <div class=\"stat-label\">Total Coverage</div>
            </div>
            <div class=\"stat\">
                <div class=\"stat-value\">~p</div>
                <div class=\"stat-label\">Modules</div>
            </div>
            <div class=\"stat\">
                <div class=\"stat-value\">~p</div>
                <div class=\"stat-label\">Total Lines</div>
            </div>
        </div>

        <div class=\"threshold ~s\">
            ~s
        </div>

        <table>
            <thead>
                <tr>
                    <th>Module</th>
                    <th>Lines</th>
                    <th>Covered</th>
                    <th>Coverage</th>
                </tr>
            </thead>
            <tbody>
                ~s
            </tbody>
        </table>
    </div>
</body>
</html>",
        [StatusClass, TotalCoverage,
         length(ModuleStats),
         lists:sum([L || {_, L, _, _} <- ModuleStats]),
         if TotalCoverage >= 80 -> "pass"; true -> "fail" end,
         if TotalCoverage >= 80 ->
            io_lib:format("✓ Coverage meets 80% threshold (~.1f%)", [TotalCoverage]);
         true ->
            io_lib:format("✗ Coverage below 80% threshold (~.1f%)", [TotalCoverage])
         end,
         ModuleRows]),

    file:write_file(IndexFile, Html).
