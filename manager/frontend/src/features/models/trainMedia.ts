const TRAIN_EDGE = 1024;
const JPEG_QUALITY = 0.88;

export const FRAME_UPLOADS = 4;
export const CAPTION_BATCH = 24;
export const PAIR_ROW_HEIGHT = 72;
export const PAIR_LIST_MAX = 512;

export function blobFromBase64(base64: string, type = 'image/png'): Blob {
  try {
    const binary = atob(base64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i += 1) {
      bytes[i] = binary.charCodeAt(i);
    }
    return new Blob([bytes], { type });
  } catch {
    return new Blob([], { type });
  }
}

function asFile(blob: Blob, filename: string): File {
  return blob instanceof File
    ? blob
    : new File([blob], filename, { type: blob.type || 'image/png' });
}

function jpegName(filename: string): string {
  const dot = filename.lastIndexOf('.');
  const stem = dot >= 0 ? filename.slice(0, dot) : filename;
  return `${stem}.jpg`;
}

export async function prepareImage(blob: Blob, filename: string): Promise<File> {
  try {
    const bitmap = await createImageBitmap(blob);
    const longest = Math.max(bitmap.width, bitmap.height);
    const scale = longest > TRAIN_EDGE ? TRAIN_EDGE / longest : 1;
    if (scale === 1 && blob.size < 200_000) {
      bitmap.close();
      return asFile(blob, filename);
    }
    const width = Math.max(1, Math.round(bitmap.width * scale));
    const height = Math.max(1, Math.round(bitmap.height * scale));
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext('2d');
    if (!context) {
      bitmap.close();
      return asFile(blob, filename);
    }
    context.drawImage(bitmap, 0, 0, width, height);
    bitmap.close();
    const resized = await new Promise<Blob | null>((resolve) => {
      canvas.toBlob(resolve, 'image/jpeg', JPEG_QUALITY);
    });
    if (!resized) return asFile(blob, filename);
    return new File([resized], jpegName(filename), { type: 'image/jpeg' });
  } catch {
    return asFile(blob, filename);
  }
}

export async function poolMap<T, R>(
  items: T[],
  limit: number,
  worker: (item: T, index: number) => Promise<R>
): Promise<R[]> {
  const results: R[] = new Array(items.length);
  let next = 0;
  const run = async () => {
    for (;;) {
      const index = next;
      next += 1;
      if (index >= items.length) return;
      results[index] = await worker(items[index], index);
    }
  };
  await Promise.all(
    Array.from({ length: Math.min(Math.max(limit, 1), items.length) }, () => run())
  );
  return results;
}

export function captionBatches<T extends { group?: number }>(
  items: T[],
  size = CAPTION_BATCH
): T[][] {
  const batches: T[][] = [];
  let current: T[] = [];
  let index = 0;
  while (index < items.length) {
    const group = items[index].group;
    let end = index + 1;
    if (group != null) {
      while (end < items.length && items[end].group === group) end += 1;
    }
    const slice = items.slice(index, end);
    if (current.length > 0 && current.length + slice.length > size) {
      batches.push(current);
      current = [];
    }
    current.push(...slice);
    index = end;
  }
  if (current.length > 0) batches.push(current);
  return batches;
}
