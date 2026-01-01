import auth/permissions
import gleeunit/should

// --- Permission Constants Tests ---

pub fn projects_permissions_are_correctly_defined_test() {
  permissions.projects_create |> should.equal("projects:create")
  permissions.projects_read |> should.equal("projects:read")
  permissions.projects_update |> should.equal("projects:update")
  permissions.projects_delete |> should.equal("projects:delete")
}

pub fn tasks_permissions_are_correctly_defined_test() {
  permissions.tasks_create |> should.equal("tasks:create")
  permissions.tasks_read |> should.equal("tasks:read")
  permissions.tasks_update |> should.equal("tasks:update")
  permissions.tasks_delete |> should.equal("tasks:delete")
}

pub fn chats_permissions_are_correctly_defined_test() {
  permissions.chats_create |> should.equal("chats:create")
  permissions.chats_read |> should.equal("chats:read")
  permissions.chats_update |> should.equal("chats:update")
  permissions.chats_delete |> should.equal("chats:delete")
}

pub fn sources_permissions_are_correctly_defined_test() {
  permissions.sources_create |> should.equal("sources:create")
  permissions.sources_read |> should.equal("sources:read")
  permissions.sources_update |> should.equal("sources:update")
  permissions.sources_delete |> should.equal("sources:delete")
}

pub fn models_permissions_are_correctly_defined_test() {
  permissions.models_create |> should.equal("models:create")
  permissions.models_read |> should.equal("models:read")
  permissions.models_update |> should.equal("models:update")
  permissions.models_delete |> should.equal("models:delete")
}

pub fn wiki_permissions_are_correctly_defined_test() {
  permissions.wiki_create |> should.equal("wiki:create")
  permissions.wiki_read |> should.equal("wiki:read")
  permissions.wiki_update |> should.equal("wiki:update")
  permissions.wiki_delete |> should.equal("wiki:delete")
}

pub fn users_permissions_are_correctly_defined_test() {
  permissions.users_create |> should.equal("users:create")
  permissions.users_read |> should.equal("users:read")
  permissions.users_update |> should.equal("users:update")
  permissions.users_delete |> should.equal("users:delete")
}

// --- For Resource Helper Tests ---

pub fn for_resource_creates_correct_format_test() {
  permissions.for_resource("chats", "read")
  |> should.equal("chats:read")

  permissions.for_resource("models", "delete")
  |> should.equal("models:delete")

  permissions.for_resource("projects", "create")
  |> should.equal("projects:create")
}

pub fn for_resource_handles_empty_strings_test() {
  permissions.for_resource("", "read")
  |> should.equal(":read")

  permissions.for_resource("chats", "")
  |> should.equal("chats:")
}

// --- Role Constants Tests ---

pub fn role_ids_are_defined_test() {
  permissions.admin_role_id
  |> should.equal("00000000-0000-0000-0000-000000000001")
  permissions.user_role_id
  |> should.equal("00000000-0000-0000-0000-000000000002")
  permissions.viewer_role_id
  |> should.equal("00000000-0000-0000-0000-000000000003")
}

pub fn role_names_are_defined_test() {
  permissions.admin_role |> should.equal("admin")
  permissions.user_role |> should.equal("user")
  permissions.viewer_role |> should.equal("viewer")
}

// --- For Method Tests ---

pub fn for_method_get_maps_to_read_test() {
  permissions.for_method("projects", "GET")
  |> should.equal("projects:read")
}

pub fn for_method_post_maps_to_create_test() {
  permissions.for_method("projects", "POST")
  |> should.equal("projects:create")
}

pub fn for_method_put_maps_to_update_test() {
  permissions.for_method("projects", "PUT")
  |> should.equal("projects:update")
}

pub fn for_method_patch_maps_to_update_test() {
  permissions.for_method("projects", "PATCH")
  |> should.equal("projects:update")
}

pub fn for_method_delete_maps_to_delete_test() {
  permissions.for_method("projects", "DELETE")
  |> should.equal("projects:delete")
}
