const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'webp']);
const VIDEO_EXTENSIONS = new Set(['mp4', 'webm', 'mov', 'm4v', 'mkv']);

type FileSystemFileEntryLike = {
  isFile: boolean;
  isDirectory: boolean;
  file: (success: (file: File) => void, error?: (error: DOMException) => void) => void;
};

type FileSystemDirectoryReaderLike = {
  readEntries: (
    success: (entries: FileSystemEntryLike[]) => void,
    error?: (error: DOMException) => void
  ) => void;
};

type FileSystemDirectoryEntryLike = {
  isFile: boolean;
  isDirectory: boolean;
  createReader: () => FileSystemDirectoryReaderLike;
};

type FileSystemEntryLike = FileSystemFileEntryLike | FileSystemDirectoryEntryLike;

export function fileExtension(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot >= 0 ? name.slice(dot + 1).toLowerCase() : '';
}

export function isImageFile(file: File): boolean {
  if (file.type === 'image/png' || file.type === 'image/jpeg' || file.type === 'image/webp') {
    return true;
  }
  if (file.type && !file.type.startsWith('image/')) return false;
  return IMAGE_EXTENSIONS.has(fileExtension(file.name));
}

export function isVideoFile(file: File): boolean {
  if (file.type.startsWith('video/')) return true;
  if (file.type) return false;
  return VIDEO_EXTENSIONS.has(fileExtension(file.name));
}

export function accepts(accept: string, file: File): boolean {
  return accept.split(',').some((pattern) => {
    const wanted = pattern.trim();
    if (!wanted) return false;
    if (wanted.endsWith('/*')) {
      const prefix = wanted.slice(0, -1);
      if (file.type.startsWith(prefix)) return true;
      if (file.type) return false;
      if (prefix === 'image/') return IMAGE_EXTENSIONS.has(fileExtension(file.name));
      if (prefix === 'video/') return VIDEO_EXTENSIONS.has(fileExtension(file.name));
      return false;
    }
    if (file.type && file.type === wanted) return true;
    if (file.type) return false;
    const extension = fileExtension(file.name);
    if (wanted === 'image/png') return extension === 'png';
    if (wanted === 'image/jpeg') return extension === 'jpg' || extension === 'jpeg';
    if (wanted === 'image/webp') return extension === 'webp';
    return false;
  });
}

function asEntry(item: DataTransferItem): FileSystemEntryLike | null {
  const getter = (
    item as DataTransferItem & {
      webkitGetAsEntry?: () => FileSystemEntryLike | null;
    }
  ).webkitGetAsEntry;
  return getter?.call(item) ?? null;
}

export async function filesFromDataTransfer(data: DataTransfer, accept: string): Promise<File[]> {
  const items = Array.from(data.items ?? []);
  const entries = items.map(asEntry).filter((entry): entry is FileSystemEntryLike => entry != null);
  const collected: File[] = [];
  if (entries.length > 0) {
    for (const entry of entries) {
      await collectEntry(entry, collected);
    }
  } else {
    collected.push(...Array.from(data.files ?? []));
  }
  return collected.filter((file) => accepts(accept, file));
}

async function collectEntry(entry: FileSystemEntryLike, into: File[]): Promise<void> {
  if (entry.isFile && 'file' in entry) {
    const file = await new Promise<File>((resolve, reject) => {
      entry.file(resolve, reject);
    });
    into.push(file);
    return;
  }
  if (entry.isDirectory && 'createReader' in entry) {
    const reader = entry.createReader();
    for (;;) {
      const batch = await new Promise<FileSystemEntryLike[]>((resolve, reject) => {
        reader.readEntries(resolve, reject);
      });
      if (batch.length === 0) break;
      for (const child of batch) {
        await collectEntry(child, into);
      }
    }
  }
}
