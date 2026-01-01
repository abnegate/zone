-module(database_connection_ffi).
-export([fixed_pool_name/0]).

%% Returns a fixed atom for the database pool name.
%% Unlike process.new_name/1 which generates unique names,
%% this always returns the same atom.
fixed_pool_name() ->
    manager_db_pool.
