export interface Attachment {
  id: string;
  name: string;
  size: number;
  type: string;
  /** Text content, read for files the model can actually be given. */
  text?: string;
  /** Data URL, for images sent to a vision model. */
  url?: string;
  /** Why this file cannot be sent, if it cannot. */
  rejected?: string;
}

// Text files are inlined into the prompt; images travel as data URLs to a
// vision model. Anything else has no path to the model, so it is surfaced as
// rejected rather than silently dropped.
const TEXT_EXTENSIONS = new Set([
  'md', 'markdown', 'txt', 'log', 'csv', 'tsv', 'json', 'jsonl', 'yaml', 'yml', 'toml', 'ini',
  'env', 'xml', 'html', 'css', 'scss', 'js', 'jsx', 'ts', 'tsx', 'py', 'rb', 'go', 'rs', 'java',
  'kt', 'swift', 'c', 'h', 'cpp', 'hpp', 'cs', 'php', 'sh', 'bash', 'zsh', 'sql', 'graphql',
  'dockerfile', 'gitignore', 'lock', 'diff', 'patch',
]);

export const MAX_ATTACHMENT_BYTES = 256 * 1024;
/// Images travel to the model as base64 data URLs, so the cap is what is
/// reasonable to put in a request body and a jsonb column, not a disk limit.
export const MAX_IMAGE_BYTES = 4 * 1024 * 1024;

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function isTextFile(file: File): boolean {
  if (file.type.startsWith('text/')) return true;
  if (/^application\/(json|xml|x-yaml|yaml|toml|javascript|typescript)$/.test(file.type)) return true;
  const extension = file.name.split('.').pop()?.toLowerCase() ?? '';
  return TEXT_EXTENSIONS.has(extension);
}

function readAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

export async function readAttachment(file: File): Promise<Attachment> {
  const base = {
    id: `${file.name}-${file.size}-${file.lastModified}`,
    name: file.name,
    size: file.size,
    type: file.type,
  };

  if (file.type.startsWith('image/')) {
    if (file.size > MAX_IMAGE_BYTES) {
      return { ...base, rejected: `over ${formatBytes(MAX_IMAGE_BYTES)}` };
    }
    try {
      return { ...base, url: await readAsDataUrl(file) };
    } catch {
      return { ...base, rejected: 'could not be read' };
    }
  }

  if (!isTextFile(file)) {
    return { ...base, rejected: 'unsupported file type' };
  }

  if (file.size > MAX_ATTACHMENT_BYTES) {
    return { ...base, rejected: `over ${formatBytes(MAX_ATTACHMENT_BYTES)}` };
  }

  try {
    return { ...base, text: await file.text() };
  } catch {
    return { ...base, rejected: 'could not be read' };
  }
}

/** Fence an attachment's content so the model sees where the file starts and ends. */
export function buildMessageWithAttachments(message: string, attachments: Attachment[]): string {
  const usable = attachments.filter((a) => a.text !== undefined);
  if (usable.length === 0) return message;

  const blocks = usable.map((a) => {
    const language = a.name.split('.').pop()?.toLowerCase() ?? '';
    return `Attached file: ${a.name}\n\`\`\`${language}\n${a.text}\n\`\`\``;
  });

  return message ? `${message}\n\n${blocks.join('\n\n')}` : blocks.join('\n\n');
}

/** Attachments the model can be given, in the shape the server stores. */
export function attachmentMetadata(attachments: Attachment[]) {
  const images = attachments.filter((a) => a.url !== undefined);
  if (images.length === 0) return undefined;
  return {
    attachments: images.map((a) => ({
      name: a.name,
      mime: a.type,
      url: a.url as string,
    })),
  };
}

export function isSendable(attachment: Attachment): boolean {
  return attachment.text !== undefined || attachment.url !== undefined;
}
