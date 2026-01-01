-module(test_db_ffi).
-export([fixed_pool_name/0, unlink_self/0]).

%% Returns a fixed atom for the test database pool name.
%% Unlike process.new_name/1 which generates unique names,
%% this always returns the same atom.
fixed_pool_name() ->
    test_db_pool_singleton.

%% Unlink the calling process from any linked processes
%% to prevent the pool from being killed when a test process terminates
unlink_self() ->
    process_flag(trap_exit, true),
    nil.
