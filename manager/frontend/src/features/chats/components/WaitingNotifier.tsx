import { useEffect } from 'react';
import { useLocation, useNavigate, useSearchParams } from 'react-router-dom';
import { useWorkspace } from '../../../shared/context';
import { registerChatOpener } from '../../../shared/notifications';
import { useWaitingNotifications } from '../hooks';

/**
 * Watches the workspace's chats for the moment one stops and tells the reader.
 *
 * Mounted in the app shell rather than on the chat list, because the reader
 * being somewhere else is the case this is for — including, in the desktop and
 * mobile client, somewhere else entirely. It renders nothing.
 *
 * It also owns what a notification does when it is clicked: the opener below is
 * what the client's host calls in the page, and what a browser notification's
 * click goes through, so a click routes to the chat without reloading the
 * console.
 */
export function WaitingNotifier() {
  const { currentWorkspace } = useWorkspace();
  const [searchParams] = useSearchParams();
  const location = useLocation();
  const navigate = useNavigate();
  const openChatId = location.pathname.startsWith('/chats') ? searchParams.get('id') : null;

  useEffect(
    () => registerChatOpener((chatId) => navigate(`/chats?id=${encodeURIComponent(chatId)}`)),
    [navigate]
  );

  useWaitingNotifications(currentWorkspace?.id ?? null, openChatId);

  return null;
}
