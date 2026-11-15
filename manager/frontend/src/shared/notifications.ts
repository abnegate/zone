/**
 * Notifications about a chat, and what a click on one does.
 *
 * Two hosts show them. In the desktop and mobile client the platform does,
 * through the client's notification plugin, so the notification outlives the
 * page: a tap brings the window forward and opens the chat even if the webview
 * was killed in between, which the client handles rather than this page. In a
 * browser the page shows it through the Notification API, and a click can only
 * do anything while the tab still exists.
 *
 * The browser never asks for permission here. A prompt nobody invited, raised
 * while the reader is doing something else, is how a reader learns to deny
 * notifications for good; in the browser the console notifies only once the
 * reader has granted it some other way, and says so rather than prompting. The
 * client does ask, because there the reader installed an app whose job is to
 * tell them when their chats stop.
 */

import { askClient, inClient } from './client';

/** What a notification is about, and what it says. */
export interface ChatNotification {
  chatId: string;
  title: string;
  body: string;
}

/** Where a click on a notification goes. */
export type ChatOpener = (chatId: string) => void;

declare global {
  // The client's host calls this in the page when a notification is tapped.
  // eslint-disable-next-line no-var
  var __zoneOpenChat: ChatOpener | undefined;
}

/**
 * Registers how this page opens a chat, and gives back the undo.
 *
 * The client's host calls `__zoneOpenChat` when a notification is tapped, which
 * is what keeps a tap from reloading the console; a click on a browser
 * notification is routed through the same function.
 */
export function registerChatOpener(open: ChatOpener): () => void {
  globalThis.__zoneOpenChat = open;
  return () => {
    if (globalThis.__zoneOpenChat === open) {
      globalThis.__zoneOpenChat = undefined;
    }
  };
}

/**
 * Whether the platform will deliver a notification, asking it if it has not
 * been asked yet.
 *
 * Desktop grants it without a prompt; Android and iOS prompt the first time and
 * remember the answer. A reading of `null` is the platform saying it has not
 * been asked, which is the only thing that prompts.
 */
async function allowedByPlatform(): Promise<boolean> {
  const granted = await askClient<boolean | null>('plugin:notification|is_permission_granted');
  if (!granted.answered) return false;
  if (typeof granted.value === 'boolean') return granted.value;
  const asked = await askClient<string>('plugin:notification|request_permission');
  return asked.answered && asked.value === 'granted';
}

/** Whether a notification would reach the reader. */
export async function notificationsAllowed(): Promise<boolean> {
  if (inClient()) return allowedByPlatform();
  return typeof Notification === 'function' && Notification.permission === 'granted';
}

/**
 * Shows one notification, reporting whether a host took it.
 *
 * The chat travels with the notification rather than being remembered here,
 * because what reads it back is the client, after a tap that can come long
 * after this page is gone.
 */
export async function notifyAboutChat(notification: ChatNotification): Promise<boolean> {
  const { chatId, title, body } = notification;
  if (inClient()) {
    const { answered } = await askClient('plugin:notification|notify', {
      options: { title, body, extra: { chat: chatId } },
    });
    return answered;
  }
  if (typeof Notification !== 'function' || Notification.permission !== 'granted') {
    return false;
  }
  try {
    // The tag collapses a chat's notifications, so a chat that stops twice
    // replaces its own notification rather than stacking another beside it.
    const shown = new Notification(title, { body, tag: chatId });
    shown.onclick = () => {
      globalThis.focus?.();
      shown.close();
      globalThis.__zoneOpenChat?.(chatId);
    };
    return true;
  } catch (error) {
    console.warn('Could not show a notification', error);
    return false;
  }
}
