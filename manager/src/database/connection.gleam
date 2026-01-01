import config
import gleam/dynamic/decode
import gleam/erlang/process
import gleam/int
import gleam/list
import gleam/option.{Some}
import gleam/otp/actor
import gleam/string
import pog

/// Database connection pool
pub type Connection =
  pog.Connection

/// Initialize the database connection pool
/// Should be called once at application startup
pub fn connect() -> Result(Connection, String) {
  let host = config.get_postgres_host()
  let port = config.get_postgres_port()
  let database = config.get_postgres_database()
  let user = config.get_postgres_user()
  let password = config.get_postgres_password()

  // Create pool name
  let pool_name = process.new_name("manager_db_pool")

  // Build configuration
  let db_config =
    pog.default_config(pool_name)
    |> pog.host(host)
    |> pog.port(port)
    |> pog.database(database)
    |> pog.user(user)
    |> pog.password(Some(password))
    |> pog.pool_size(10)

  // Start the connection pool
  case pog.start(db_config) {
    Ok(actor.Started(_, conn)) -> Ok(conn)
    Error(actor.InitTimeout) -> Error("Database connection timeout")
    Error(actor.InitFailed(msg)) -> Error("Database connection failed: " <> msg)
    Error(actor.InitExited(_)) -> Error("Database connection exited")
  }
}

/// Get a connection by name (for use after pool is started)
pub fn named_connection() -> Connection {
  let pool_name = process.new_name("manager_db_pool")
  pog.named_connection(pool_name)
}

/// Convert pog query error to string
pub fn query_error_to_string(err: pog.QueryError) -> String {
  case err {
    pog.ConnectionUnavailable -> "Database connection unavailable"
    pog.PostgresqlError(code, name, message) ->
      "PostgreSQL error [" <> code <> "/" <> name <> "]: " <> message
    pog.UnexpectedArgumentCount(expected, got) ->
      "Unexpected argument count: expected "
      <> int.to_string(expected)
      <> ", got "
      <> int.to_string(got)
    pog.UnexpectedArgumentType(expected, got) ->
      "Unexpected argument type: expected " <> expected <> ", got " <> got
    pog.UnexpectedResultType(errors) -> {
      let error_strs =
        list.map(errors, fn(e) {
          case e {
            decode.DecodeError(expected, found, path) ->
              "expected "
              <> expected
              <> ", found "
              <> found
              <> " at "
              <> string.join(path, ".")
          }
        })
      "Unexpected result type: " <> string.join(error_strs, "; ")
    }
    pog.ConstraintViolated(message, constraint, detail) ->
      "Constraint violated ["
      <> constraint
      <> "]: "
      <> message
      <> " - "
      <> detail
    pog.QueryTimeout -> "Query timeout"
  }
}
