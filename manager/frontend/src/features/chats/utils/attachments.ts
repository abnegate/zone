import { cancelled, isCancelled } from '../../../api/upload';

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
  /** Reused chat image that should be the img2img starting point. */
  source?: boolean;
}

// Text files are inlined into the prompt; images travel as data URLs to a
// vision model. Anything else has no path to the model, so it is surfaced as
// rejected rather than silently dropped.
const TEXT_EXTENSIONS = new Set([
  'md',
  'markdown',
  'txt',
  'log',
  'csv',
  'tsv',
  'json',
  'jsonl',
  'yaml',
  'yml',
  'toml',
  'ini',
  'env',
  'xml',
  'html',
  'css',
  'scss',
  'js',
  'jsx',
  'ts',
  'tsx',
  'py',
  'rb',
  'go',
  'rs',
  'java',
  'kt',
  'swift',
  'c',
  'h',
  'cpp',
  'hpp',
  'cs',
  'php',
  'sh',
  'bash',
  'zsh',
  'sql',
  'graphql',
  'dockerfile',
  'gitignore',
  'lock',
  'diff',
  'patch',
]);

export const MAX_ATTACHMENT_BYTES = 256 * 1024;
/// Images travel to the model as base64 data URLs, so the cap is what is
/// reasonable to put in a request body and a jsonb column, not a disk limit.
export const MAX_IMAGE_BYTES = 4 * 1024 * 1024;
/// A phone photo is routinely bigger than a data URL should be, and a HEIC one
/// is resampled before it becomes a URL, so its source is held to a wider cap.
export const MAX_SOURCE_IMAGE_BYTES = 16 * 1024 * 1024;
/// Long edge a transcoded photo is sampled down to. Vision models read about
/// this many pixels, and it keeps the re-encoded JPEG inside MAX_IMAGE_BYTES.
export const MAX_IMAGE_EDGE = 1568;
const JPEG_QUALITY = 0.9;

// No model takes HEIC and no engine but WebKit draws it, so a HEIC attachment
// is decoded and re-encoded as JPEG rather than previewed and sent as it came.
const HEIC_TYPES = new Set([
  'image/heic',
  'image/heif',
  'image/heic-sequence',
  'image/heif-sequence',
]);
const HEIC_EXTENSIONS = new Set(['heic', 'heif', 'hif']);

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function isTextFile(file: File): boolean {
  if (file.type.startsWith('text/')) return true;
  if (/^application\/(json|xml|x-yaml|yaml|toml|javascript|typescript)$/.test(file.type))
    return true;
  const extension = extensionOf(file.name);
  return TEXT_EXTENSIONS.has(extension);
}

function extensionOf(name: string): string {
  return name.split('.').pop()?.toLowerCase() ?? '';
}

/** A phone's HEIC arrives with no type at all on platforms that do not map it,
 * where the type alone would have it rejected as an unsupported file. */
export function isHeicFile(file: { name: string; type: string }): boolean {
  const type = file.type.toLowerCase();
  if (HEIC_TYPES.has(type)) return true;
  if (type && type !== 'application/octet-stream') return false;
  return HEIC_EXTENSIONS.has(extensionOf(file.name));
}

/** The name the transcoded attachment carries, since it is no longer a HEIC. */
export function jpegName(name: string): string {
  const dot = name.lastIndexOf('.');
  return `${dot > 0 ? name.slice(0, dot) : name}.jpg`;
}

/** The transcode path stamps image/jpeg and a .jpg name on what it returns, so
 * a base64 JPEG data URL is the only thing a transcoder may hand back. */
export function isJpegDataUrl(url: string): boolean {
  return url.startsWith('data:image/jpeg;base64,');
}

/** Bytes behind a base64 data URL, which is what the size cap is really about.
 * A string that is not one has no payload to measure and counts as nothing. */
export function dataUrlBytes(url: string): number {
  const comma = url.indexOf(',');
  if (comma < 0) return 0;
  const base64 = url.slice(comma + 1);
  const padding = base64.endsWith('==') ? 2 : base64.endsWith('=') ? 1 : 0;
  return Math.max(0, Math.floor((base64.length * 3) / 4) - padding);
}

/** Canvas size for a bitmap held to a long edge, never smaller than a pixel. */
export function fitWithin(width: number, height: number, edge = MAX_IMAGE_EDGE) {
  const longest = Math.max(width, height, 1);
  const scale = Math.min(1, edge / longest);
  return {
    width: Math.max(1, Math.round(width * scale)),
    height: Math.max(1, Math.round(height * scale)),
  };
}

/** Decode-and-re-encode seam: null when this engine has no decoder for the file. */
export type Transcoder = (file: Blob) => Promise<string | null>;

export const toJpegDataUrl: Transcoder = async (file) => {
  if (typeof createImageBitmap !== 'function') return null;
  let bitmap: ImageBitmap;
  try {
    // A phone photo carries its rotation in EXIF. from-image is the spec's
    // default, but WebKit is the engine that reaches this path, and it read
    // the metadata only once the default changed, so say it rather than assume.
    bitmap = await createImageBitmap(file, { imageOrientation: 'from-image' });
  } catch {
    return null;
  }
  try {
    const { width, height } = fitWithin(bitmap.width, bitmap.height);
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext('2d');
    if (!context) return null;
    context.drawImage(bitmap, 0, 0, width, height);
    // A canvas with no JPEG encoder hands back the PNG it defaults to.
    const url = canvas.toDataURL('image/jpeg', JPEG_QUALITY);
    return isJpegDataUrl(url) ? url : null;
  } catch {
    return null;
  } finally {
    bitmap.close();
  }
};

type ReadOptions = {
  signal?: AbortSignal;
  onProgress?: (fraction: number) => void;
  transcode?: Transcoder;
};

const ReadFormat = {
  Text: 'text',
  DataUrl: 'data-url',
} as const;

type ReadFormat = (typeof ReadFormat)[keyof typeof ReadFormat];

function read(file: File, format: ReadFormat, options: ReadOptions): Promise<string> {
  const { signal, onProgress } = options;
  return new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(cancelled());
      return;
    }
    const reader = new FileReader();
    const abort = () => reader.abort();
    signal?.addEventListener('abort', abort, { once: true });
    const settle = () => signal?.removeEventListener('abort', abort);
    reader.onprogress = (event) => {
      if (event.lengthComputable && event.total > 0) onProgress?.(event.loaded / event.total);
    };
    reader.onload = () => {
      settle();
      resolve(String(reader.result));
    };
    reader.onerror = () => {
      settle();
      reject(reader.error);
    };
    reader.onabort = () => {
      settle();
      reject(cancelled());
    };
    if (format === ReadFormat.DataUrl) {
      reader.readAsDataURL(file);
    } else {
      reader.readAsText(file);
    }
  });
}

export function attachmentId(file: File): string {
  return `${file.name}-${file.size}-${file.lastModified}`;
}

export async function readAttachment(file: File, options: ReadOptions = {}): Promise<Attachment> {
  const base = {
    id: attachmentId(file),
    name: file.name,
    size: file.size,
    type: file.type,
  };

  if (isHeicFile(file)) {
    return readTranscoded(file, base, options);
  }

  if (file.type.startsWith('image/')) {
    if (file.size > MAX_IMAGE_BYTES) {
      return { ...base, rejected: `over ${formatBytes(MAX_IMAGE_BYTES)}` };
    }
    try {
      return { ...base, url: await read(file, ReadFormat.DataUrl, options) };
    } catch (error) {
      if (isCancelled(error)) throw error;
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
    return { ...base, text: await read(file, ReadFormat.Text, options) };
  } catch (error) {
    if (isCancelled(error)) throw error;
    return { ...base, rejected: 'could not be read' };
  }
}

/** Re-encode an image the model and the browser cannot take as it came. The
 * file is decoded whole rather than streamed, so progress only has two points,
 * and a decode already under way runs to the end whatever the signal says:
 * cancelling throws its result away rather than stopping the work. */
async function readTranscoded(
  file: File,
  base: { id: string; name: string; size: number; type: string },
  options: ReadOptions
): Promise<Attachment> {
  const { signal, onProgress, transcode = toJpegDataUrl } = options;
  if (signal?.aborted) throw cancelled();
  if (file.size > MAX_SOURCE_IMAGE_BYTES) {
    return { ...base, rejected: `over ${formatBytes(MAX_SOURCE_IMAGE_BYTES)}` };
  }

  const url = await transcode(file);
  if (signal?.aborted) throw cancelled();
  if (!url) {
    return { ...base, rejected: 'HEIC needs converting to JPEG' };
  }
  if (!isJpegDataUrl(url)) {
    return { ...base, rejected: 'could not be read' };
  }

  const size = dataUrlBytes(url);
  if (size > MAX_IMAGE_BYTES) {
    return { ...base, rejected: `over ${formatBytes(MAX_IMAGE_BYTES)}` };
  }

  onProgress?.(1);
  return { ...base, name: jpegName(file.name), type: 'image/jpeg', size, url };
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

/** Reuse an image already on the thread as the next generation's source. */
export function sourceAttachment(image: { name: string; mime: string; url: string }): Attachment {
  return {
    id: `source:${image.url}`,
    name: image.name,
    size: 0,
    type: image.mime,
    url: image.url,
    source: true,
  };
}

export function isStartingImage(attachment: Attachment): boolean {
  return Boolean(attachment.source && attachment.url);
}

/** Images stored on a message so the thread can render them after send. */
export function imageAttachments(
  metadata?: { attachments?: { name: string; mime: string; url: string }[] } | null
) {
  return metadata?.attachments?.filter((a) => a.mime.startsWith('image/') && Boolean(a.url)) ?? [];
}

/** Videos stored on a message so the thread can render them after send. */
export function videoAttachments(
  metadata?: { attachments?: { name: string; mime: string; url: string }[] } | null
) {
  return metadata?.attachments?.filter((a) => a.mime.startsWith('video/') && Boolean(a.url)) ?? [];
}

/** Audio stored on a message so the thread can play it after send. */
export function audioAttachments(
  metadata?: { attachments?: { name: string; mime: string; url: string }[] } | null
) {
  return metadata?.attachments?.filter((a) => a.mime.startsWith('audio/') && Boolean(a.url)) ?? [];
}
