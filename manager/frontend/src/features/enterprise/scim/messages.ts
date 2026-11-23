import { NAME_REQUIRED } from './labels';
import { ScimCode } from './schemas';

const MESSAGES: Readonly<Record<ScimCode, string>> = {
  [ScimCode.LicenseRequired]:
    'This needs a Zone Enterprise license that is active or in its grace period.',
  [ScimCode.TokenNotFound]:
    'This token no longer exists. It may have been revoked somewhere else. Reload the page.',
  [ScimCode.GroupNotFound]:
    'Your identity provider has deleted this group since the page loaded. Reload the page.',
  [ScimCode.TokenNameInvalid]: NAME_REQUIRED,
  [ScimCode.MappingInvalid]: 'Choose Admin, Member or one of your custom roles for this group.',
  [ScimCode.OwnerUnmappable]: "A group can't make people owners. Choose Admin or Member.",
  [ScimCode.RoleNotFound]:
    'That custom role no longer exists or no longer applies to the organization. Reload the page.',
  [ScimCode.Exceeds]:
    "That role includes permissions you don't hold, so you can't give it to a group.",
};

export function refusalMessage(code: ScimCode): string {
  return MESSAGES[code];
}
