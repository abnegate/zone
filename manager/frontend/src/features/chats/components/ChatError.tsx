import { Link } from 'react-router-dom';
import { AGENT_POLICY_LINK, denialMessage, type PolicyDenialCode } from '../../enterprise';
import { SettingsGroup, SettingsSection, settingsPath } from '../../settings/navigation/sections';

export const AGENT_POLICY_PATH = settingsPath(SettingsGroup.Workspace, SettingsSection.AgentPolicy);

interface ChatErrorProps {
  message: string;
  denial: PolicyDenialCode | null;
}

export function ChatError({ message, denial }: ChatErrorProps) {
  if (denial === null) {
    return (
      <div className="chats-error" role="alert">
        {message}
      </div>
    );
  }
  return (
    <div className="chats-error chats-denial" role="alert" data-testid="chat-denial">
      <p className="chats-denial-message">{message.trim() || denialMessage(denial)}</p>
      <Link className="chats-denial-link" to={AGENT_POLICY_PATH}>
        {AGENT_POLICY_LINK}
      </Link>
    </div>
  );
}
