import auth/jwt
import auth/password
import auth/permissions
import config
import database/queries/refresh_tokens
import database/queries/users
import gleam/http
import gleam/http/request
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/string
import models/user
import web.{type Context}
import wisp.{type Request, type Response}

/// Handle all /api/auth routes
pub fn handle_auth_route(
  req: Request,
  path: List(String),
  ctx: Context,
) -> Response {
  case path {
    ["register"] -> handle_register(req, ctx)
    ["login"] -> handle_login(req, ctx)
    ["refresh"] -> handle_refresh(req, ctx)
    ["logout"] -> handle_logout(req, ctx)
    _ -> wisp.not_found()
  }
}

/// POST /api/auth/register - Register new user
fn handle_register(req: Request, ctx: Context) -> Response {
  case req.method {
    http.Post -> {
      use body <- wisp.require_string_body(req)

      case user.decode_register_request(body) {
        Ok(register_req) -> {
          // Validate email format (basic check)
          case string.contains(register_req.email, "@") {
            False -> web.bad_request("Invalid email format")
            True -> {
              // Validate password length
              case string.length(register_req.password) >= 8 {
                False ->
                  web.bad_request("Password must be at least 8 characters")
                True -> {
                  // Try to create user - use unique constraint to handle race condition
                  // The first user to successfully insert becomes admin
                  // Hash password before attempting to create
                  let password_hash = password.hash_password(register_req.password)

                  case users.count_users(ctx.db) {
                    Ok(0) -> {
                      // Attempt to create as admin
                      case
                        users.create_user(
                          ctx.db,
                          register_req.email,
                          password_hash,
                          register_req.display_name,
                          True,
                        )
                      {
                        Ok(new_user) ->
                          complete_user_registration(
                            req,
                            ctx,
                            new_user,
                            permissions.admin_role,
                          )
                        Error(err) -> {
                          // If creation failed due to unique constraint, user already exists
                          case
                            string.contains(err, "unique")
                            || string.contains(err, "duplicate")
                            || string.contains(err, "already exists")
                          {
                            True -> {
                              // Another user was created first, retry as regular user
                              case
                                users.create_user(
                                  ctx.db,
                                  register_req.email,
                                  password_hash,
                                  register_req.display_name,
                                  False,
                                )
                              {
                                Ok(new_user) ->
                                  complete_user_registration(
                                    req,
                                    ctx,
                                    new_user,
                                    permissions.user_role,
                                  )
                                Error(err2) -> {
                                  case
                                    string.contains(err2, "unique")
                                    || string.contains(err2, "duplicate")
                                  {
                                    True -> web.json_error(409, "Email already registered")
                                    False -> web.internal_error(err2)
                                  }
                                }
                              }
                            }
                            False -> web.internal_error(err)
                          }
                        }
                      }
                    }
                    Ok(_) -> {
                      // Regular user creation
                      case
                        users.create_user(
                          ctx.db,
                          register_req.email,
                          password_hash,
                          register_req.display_name,
                          False,
                        )
                      {
                        Ok(new_user) ->
                          complete_user_registration(
                            req,
                            ctx,
                            new_user,
                            permissions.user_role,
                          )
                        Error(err) -> {
                          case
                            string.contains(err, "unique")
                            || string.contains(err, "duplicate")
                            || string.contains(err, "already exists")
                          {
                            True -> web.json_error(409, "Email already registered")
                            False -> web.internal_error(err)
                          }
                        }
                      }
                    }
                    Error(err) -> web.internal_error(err)
                  }
                }
              }
            }
          }
        }
        Error(_) -> web.bad_request("Invalid request body")
      }
    }
    _ -> wisp.method_not_allowed([http.Post])
  }
}

/// Complete user registration by assigning role and generating tokens
fn complete_user_registration(
  req: Request,
  ctx: Context,
  new_user: user.User,
  role_name: String,
) -> Response {
  // Assign role (user self-assigns during registration)
  case users.assign_role(ctx.db, new_user.id, role_name, new_user.id) {
    Ok(_) -> {
      // Generate tokens
      case generate_auth_response(ctx, new_user.id, req) {
        Ok(auth_resp) ->
          web.json_created([#("data", user.auth_response_to_json(auth_resp))])
        Error(err) -> web.internal_error(err)
      }
    }
    Error(err) -> web.internal_error(err)
  }
}

/// POST /api/auth/login - Login user
fn handle_login(req: Request, ctx: Context) -> Response {
  case req.method {
    http.Post -> {
      use body <- wisp.require_string_body(req)

      case user.decode_login_request(body) {
        Ok(login_req) -> {
          case users.get_user_by_email(ctx.db, login_req.email) {
            Ok(Some(#(user_record, password_hash))) -> {
              case user_record.is_active {
                False -> web.json_error(403, "Account is disabled")
                True -> {
                  case
                    password.verify_password(login_req.password, password_hash)
                  {
                    True -> {
                      // Update last login
                      let _ = users.update_last_login(ctx.db, user_record.id)

                      // Generate tokens
                      case generate_auth_response(ctx, user_record.id, req) {
                        Ok(auth_resp) ->
                          web.json_success([
                            #("data", user.auth_response_to_json(auth_resp)),
                          ])
                        Error(err) -> web.internal_error(err)
                      }
                    }
                    False -> web.json_error(401, "Invalid credentials")
                  }
                }
              }
            }
            Ok(None) -> web.json_error(401, "Invalid credentials")
            Error(err) -> web.internal_error(err)
          }
        }
        Error(_) -> web.bad_request("Invalid request body")
      }
    }
    _ -> wisp.method_not_allowed([http.Post])
  }
}

/// POST /api/auth/refresh - Refresh access token
fn handle_refresh(req: Request, ctx: Context) -> Response {
  case req.method {
    http.Post -> {
      use body <- wisp.require_string_body(req)

      case user.decode_refresh_request(body) {
        Ok(refresh_req) -> {
          case
            refresh_tokens.validate_refresh_token(
              ctx.db,
              refresh_req.refresh_token,
            )
          {
            Ok(Some(user_id)) -> {
              // Revoke old refresh token (token rotation)
              let _ =
                refresh_tokens.revoke_refresh_token(
                  ctx.db,
                  refresh_req.refresh_token,
                )

              // Generate new tokens
              case generate_auth_response(ctx, user_id, req) {
                Ok(auth_resp) ->
                  web.json_success([
                    #("data", user.auth_response_to_json(auth_resp)),
                  ])
                Error(err) -> web.internal_error(err)
              }
            }
            Ok(None) -> web.json_error(401, "Invalid or expired refresh token")
            Error(err) -> web.internal_error(err)
          }
        }
        Error(_) -> web.bad_request("Invalid request body")
      }
    }
    _ -> wisp.method_not_allowed([http.Post])
  }
}

/// POST /api/auth/logout - Logout (revoke refresh token)
fn handle_logout(req: Request, ctx: Context) -> Response {
  case req.method {
    http.Post -> {
      use body <- wisp.require_string_body(req)

      case user.decode_refresh_request(body) {
        Ok(refresh_req) -> {
          let _ =
            refresh_tokens.revoke_refresh_token(
              ctx.db,
              refresh_req.refresh_token,
            )
          wisp.no_content()
        }
        // Don't reveal if token was valid
        Error(_) -> wisp.no_content()
      }
    }
    _ -> wisp.method_not_allowed([http.Post])
  }
}

/// Generate access and refresh tokens for a user
fn generate_auth_response(
  ctx: Context,
  user_id: String,
  req: Request,
) -> Result(user.AuthResponse, String) {
  let jwt_secret = config.get_jwt_secret()
  let access_lifetime = config.get_jwt_access_lifetime()
  let refresh_lifetime = config.get_jwt_refresh_lifetime()

  case users.get_user_with_permissions(ctx.db, user_id) {
    Ok(Some(user_with_perms)) -> {
      // Create access token
      let access_token =
        jwt.create_access_token(
          user_id,
          user_with_perms.user.email,
          user_with_perms.roles,
          user_with_perms.permissions,
          jwt_secret,
          access_lifetime,
        )

      // Create refresh token
      let #(refresh_token, expires_at) =
        jwt.create_refresh_token(refresh_lifetime)

      // Store refresh token
      let user_agent =
        request.get_header(req, "user-agent") |> option.from_result
      let ip_address = get_client_ip(req)

      case
        refresh_tokens.create_refresh_token(
          ctx.db,
          user_id,
          refresh_token,
          expires_at,
          user_agent,
          ip_address,
        )
      {
        Ok(_) -> {
          Ok(user.AuthResponse(
            access_token: access_token,
            refresh_token: refresh_token,
            expires_in: access_lifetime,
            user: user_with_perms.user,
            roles: user_with_perms.roles,
            permissions: user_with_perms.permissions,
          ))
        }
        Error(err) -> Error(err)
      }
    }
    Ok(None) -> Error("User not found")
    Error(err) -> Error(err)
  }
}

fn get_client_ip(req: Request) -> Option(String) {
  // Check X-Forwarded-For first (for reverse proxy)
  case request.get_header(req, "x-forwarded-for") {
    Ok(forwarded) -> {
      case string.split(forwarded, ",") |> list.first {
        Ok(ip) -> Some(string.trim(ip))
        Error(_) -> None
      }
    }
    Error(_) -> {
      case request.get_header(req, "x-real-ip") {
        Ok(ip) -> Some(ip)
        Error(_) -> None
      }
    }
  }
}
