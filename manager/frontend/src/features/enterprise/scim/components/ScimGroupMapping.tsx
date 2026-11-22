import { Select } from '@zone/ui';
import type { CustomRoleReference } from '../../roles/types';
import { useMapScimGroup, useUnmapScimGroup } from '../hooks';
import { mappingLabel } from '../labels';
import { mappingOptions, mappingRequest, mappingValue } from '../mapping';
import { refusalMessage } from '../messages';
import type { GroupResponse } from '../types';

interface ScimGroupMappingProps {
  organizationId: string;
  group: GroupResponse;
  offered: readonly CustomRoleReference[];
  editable: boolean;
}

export function ScimGroupMapping({
  organizationId,
  group,
  offered,
  editable,
}: ScimGroupMappingProps) {
  const mapping = useMapScimGroup(organizationId);
  const unmapping = useUnmapScimGroup(organizationId);
  const pending = mapping.pending || unmapping.pending;
  const refusal = mapping.refusal ?? unmapping.refusal;
  const failure = refusal ? refusalMessage(refusal.code) : (mapping.error ?? unmapping.error);

  const choose = (value: string) => {
    mapping.reset();
    unmapping.reset();
    const request = mappingRequest(value);
    if (request === null) {
      unmapping.unmap(group.id);
    } else {
      mapping.map(group.id, request);
    }
  };

  return (
    <div className="scim-mapping">
      <Select
        compact
        aria-label={mappingLabel(group.display_name)}
        options={mappingOptions(offered, group.custom_role)}
        value={mappingValue(group)}
        disabled={!editable || pending}
        onValueChange={choose}
      />
      {failure && (
        <p className="scim-field-error" role="alert">
          {failure}
        </p>
      )}
    </div>
  );
}
