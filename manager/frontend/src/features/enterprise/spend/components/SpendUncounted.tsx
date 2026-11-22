import { Link } from 'react-router-dom';
import { SectionCard } from '../../../analytics/components/SectionCard';
import {
  SettingsGroup,
  SettingsSection,
  settingsPath,
} from '../../../settings/navigation/sections';
import {
  AI_SETTINGS_LINK,
  UNCOUNTED_DESCRIPTION,
  UNCOUNTED_FOOTNOTE,
  UNCOUNTED_PRESENT,
  UNCOUNTED_TITLE,
} from '../labels';
import { uncountedLine } from '../messages';
import type { UncountedReason } from '../schemas';

export const AI_SETTINGS_PATH = settingsPath(SettingsGroup.Organization, SettingsSection.Ai);

interface SpendUncountedProps {
  reasons: readonly UncountedReason[];
}

export function SpendUncounted({ reasons }: SpendUncountedProps) {
  return (
    <SectionCard title={UNCOUNTED_TITLE} description={UNCOUNTED_DESCRIPTION} wide>
      <div className="spend-uncounted">
        {reasons.length > 0 && (
          <>
            <p className="spend-uncounted-lead">{UNCOUNTED_PRESENT}</p>
            <ul className="spend-uncounted-list" aria-label={UNCOUNTED_PRESENT}>
              {reasons.map((reason) => (
                <li key={reason}>{uncountedLine(reason)}</li>
              ))}
            </ul>
          </>
        )}
        <p className="spend-footnote">{UNCOUNTED_FOOTNOTE}</p>
        <Link to={AI_SETTINGS_PATH} className="spend-link">
          {AI_SETTINGS_LINK}
        </Link>
      </div>
    </SectionCard>
  );
}
