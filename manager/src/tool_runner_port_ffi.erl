%% Tool Runner Port FFI
%% Erlang port interface for communicating with the Rust runner process
-module(tool_runner_port_ffi).
-export([
    open_port/2,
    send_to_port/2,
    receive_from_port/2,
    close_port/1,
    is_port_alive/1
]).

%% Open a port to the runner binary
%% Returns {ok, Port} or {error, Reason}
open_port(BinaryPath, Args) ->
    try
        BinaryPathBin = unicode:characters_to_binary(BinaryPath),
        ArgsBin = [unicode:characters_to_binary(A) || A <- Args],
        Port = erlang:open_port(
            {spawn_executable, BinaryPathBin},
            [
                {args, ArgsBin},
                binary,
                {line, 65536},  % Line-based for NDJSON
                use_stdio,
                exit_status,
                hide  % Don't show window on Windows
            ]
        ),
        {ok, Port}
    catch
        error:Reason ->
            {error, io_lib:format("Failed to open port: ~p", [Reason])}
    end.

%% Send data to the port
%% Data should be a binary ending with \n
send_to_port(Port, Data) ->
    try
        DataBin = unicode:characters_to_binary(Data),
        erlang:port_command(Port, DataBin),
        ok
    catch
        error:Reason ->
            {error, io_lib:format("Failed to send: ~p", [Reason])}
    end.

%% Receive a line from the port with timeout
%% Returns {ok, Line} or {error, timeout} or {error, exit}
receive_from_port(Port, TimeoutMs) ->
    receive
        {Port, {data, {eol, Line}}} ->
            {ok, unicode:characters_to_list(Line)};
        {Port, {data, {noeol, Partial}}} ->
            receive_more(Port, Partial, TimeoutMs);
        {Port, {exit_status, Status}} ->
            {error, {exit, Status}}
    after TimeoutMs ->
        {error, timeout}
    end.

%% Continue receiving partial lines
receive_more(Port, Acc, TimeoutMs) ->
    receive
        {Port, {data, {eol, Line}}} ->
            {ok, unicode:characters_to_list(<<Acc/binary, Line/binary>>)};
        {Port, {data, {noeol, Partial}}} ->
            receive_more(Port, <<Acc/binary, Partial/binary>>, TimeoutMs);
        {Port, {exit_status, Status}} ->
            {error, {exit, Status}}
    after TimeoutMs ->
        {error, timeout}
    end.

%% Close the port
close_port(Port) ->
    try
        erlang:port_close(Port),
        ok
    catch
        error:_ -> ok  % Already closed
    end.

%% Check if port is still alive
is_port_alive(Port) ->
    try
        erlang:port_info(Port) =/= undefined
    catch
        error:_ -> false
    end.
