import { Button, Checkbox, Input, Select } from '@zone/ui';
import { type FormEvent, useEffect, useState } from 'react';
import { modelsApi } from '../../../api/models';

type TrainBase = { id: string; label: string; edit: boolean };

type TrainImage = {
  id: string;
  filename: string;
  caption: string;
  bytes_base64: string;
  before_base64?: string;
  group?: number;
  source?: string;
  mirrored?: boolean;
};

function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = String(reader.result || '');
      const encoded = result.includes(',') ? result.slice(result.indexOf(',') + 1) : result;
      resolve(encoded);
    };
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

// Frames of one clip are all named alike, so the clip they came from is what
// tells them apart, and a mirrored one is worth saying so its caption can allow
// for it.
function captionOf(image: TrainImage): string {
  const named = image.source ? `${image.source} ${image.filename}` : image.filename;
  return image.mirrored ? `${named} (mirrored)` : named;
}

// Frames arrive grouped per clip, so a second clip has to be shifted past the
// groups already on the list or the two clips would be captioned as one.
function nextGroup(images: TrainImage[]): number {
  return images.reduce((highest, image) => Math.max(highest, (image.group ?? -1) + 1), 0);
}

export default function TrainPanel({ onTrained }: { onTrained: () => void }) {
  const [bases, setBases] = useState<TrainBase[]>([]);
  const [name, setName] = useState('');
  const [base, setBase] = useState('');
  const [trigger, setTrigger] = useState('');
  const [images, setImages] = useState<TrainImage[]>([]);
  const [mirror, setMirror] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [captioning, setCaptioning] = useState(false);
  const [sampling, setSampling] = useState<string | null>(null);
  const [sampled, setSampled] = useState<string | null>(null);

  useEffect(() => {
    modelsApi
      .trainBases()
      .then((rows) => {
        setBases(rows);
        setBase((current) => current || rows[0]?.id || '');
      })
      .catch(() => setBases([]));
  }, []);

  const selected = bases.find((row) => row.id === base);
  const edit = Boolean(selected?.edit);

  const handleFiles = async (files: FileList | null, before = false) => {
    if (!files?.length) return;
    const encoded = await Promise.all(
      Array.from(files).map(async (file) => ({ name: file.name, bytes: await fileToBase64(file) }))
    );
    setImages((current) => {
      const next = [...current];
      for (const file of encoded) {
        if (before) {
          const index = next.findIndex((image) => !image.before_base64);
          if (index >= 0) {
            next[index] = { ...next[index], before_base64: file.bytes };
          }
        } else {
          next.push({
            id: crypto.randomUUID(),
            filename: file.name,
            caption: '',
            bytes_base64: file.bytes,
          });
        }
      }
      return next;
    });
  };

  const handleVideos = async (files: FileList | null) => {
    if (!files?.length) return;
    setError(null);
    setSampled(null);
    try {
      for (const file of Array.from(files)) {
        setSampling(file.name);
        const clip = await modelsApi.frames({
          filename: file.name,
          bytes_base64: await fileToBase64(file),
          mirror,
        });
        // Appended against whatever the list holds now, not against a copy
        // taken before the upload: images picked while a clip was extracting
        // would otherwise be dropped, and a clip that failed would take the
        // frames of the clips before it with it.
        setImages((current) => {
          const offset = nextGroup(current);
          return [
            ...current,
            ...clip.frames.map((frame) => ({
              id: crypto.randomUUID(),
              filename: frame.filename,
              caption: '',
              bytes_base64: frame.bytes_base64,
              group: offset + frame.group,
              source: file.name,
              mirrored: frame.mirrored,
            })),
          ];
        });
        setSampled(
          `${file.name}: ${clip.sampled} frames read at ${clip.sampled_fps.toFixed(1)}/s, ${clip.frames.length} kept`
        );
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not read the video');
    } finally {
      setSampling(null);
    }
  };

  const handleCaption = async () => {
    if (images.length === 0) return;
    setCaptioning(true);
    setError(null);
    try {
      const { captions } = await modelsApi.captions({
        trigger: trigger.trim() || undefined,
        images: images.map(({ filename, caption, bytes_base64, group }) => ({
          filename,
          caption,
          bytes_base64,
          group,
        })),
      });
      setImages((current) =>
        current.map((image, index) => ({ ...image, caption: captions[index] ?? image.caption }))
      );
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Captioning failed');
    } finally {
      setCaptioning(false);
    }
  };

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (!name.trim() || !base || !trigger.trim() || images.length === 0) return;
    setBusy(true);
    setError(null);
    try {
      await modelsApi.train({
        name: name.trim(),
        base,
        trigger: trigger.trim() || undefined,
        images: images.map(({ filename, caption, bytes_base64, before_base64, group }) => ({
          filename,
          caption,
          bytes_base64,
          before_base64,
          group,
        })),
      });
      setImages([]);
      setName('');
      setSampled(null);
      onTrained();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Training failed');
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="card">
      <h2>Train a LoRA</h2>
      <p className="help-text">
        Drop images or a video, pick an installed base, and set a unique trigger word. Every image
        is cropped square on its subject, then Zone trains every transformer block (rank 32, alpha
        equals rank, 400+ steps) so the LoRA can keep that identity.
      </p>
      {error && <div className="error-placeholder">{error}</div>}
      <form className="ui-form" onSubmit={handleSubmit}>
        <Input
          label="Name"
          value={name}
          onChange={(event) => setName(event.target.value)}
          required
        />
        <Select
          label="Base"
          value={base}
          onValueChange={setBase}
          options={bases.map((row) => ({ value: row.id, label: row.label }))}
          placeholder="No trainable base installed"
          disabled={bases.length === 0}
        />
        <Input
          label="Trigger word"
          value={trigger}
          onChange={(event) => setTrigger(event.target.value)}
          placeholder="required for a unique identity"
          required
        />
        <Input
          label="Images"
          type="file"
          accept="image/png,image/jpeg,image/webp"
          multiple
          onChange={(event) => void handleFiles(event.target.files)}
        />
        {edit && (
          <Input
            label="Before images (edit bases)"
            type="file"
            accept="image/png,image/jpeg,image/webp"
            multiple
            onChange={(event) => void handleFiles(event.target.files, true)}
          />
        )}
        <Input
          label="Video"
          type="file"
          accept="video/*"
          multiple
          disabled={Boolean(sampling)}
          onChange={(event) => void handleVideos(event.target.files)}
        />
        <p className="help-text">
          A clip is sampled above the rate it keeps, so the sharpest frame of each moment wins its
          slot, repeats of a shot already taken are dropped, and every frame is cropped around
          whatever moved.
        </p>
        <Checkbox
          label="Mirror half the frames of each second"
          helpText="More variety from one angle, applied as each clip is read. Turn it off for a subject carrying text, or one a mirror would get wrong."
          checked={mirror}
          onCheckedChange={setMirror}
        />
        {sampling && <p className="help-text">Reading {sampling}…</p>}
        {sampled && <p className="help-text">{sampled}</p>}
        {images.length > 0 && (
          <div>
            <Button
              type="button"
              variant="secondary"
              loading={captioning}
              disabled={captioning}
              onClick={() => void handleCaption()}
            >
              Auto-caption images
            </Button>
            <p className="help-text">
              Describes pose, setting, and lighting only, so the trigger word carries the identity.
              Captions you have written are kept, and frames of one shot are described once.
            </p>
          </div>
        )}
        {images.map((image, index) => (
          <Input
            key={image.id}
            id={`train-caption-${index}`}
            label={`Caption for ${captionOf(image)}`}
            value={image.caption}
            onChange={(event) => {
              const caption = event.target.value;
              setImages((current) =>
                current.map((row) => (row.id === image.id ? { ...row, caption } : row))
              );
            }}
          />
        ))}
        <Button
          type="submit"
          loading={busy}
          disabled={
            !name.trim() || !base || !trigger.trim() || images.length === 0 || Boolean(sampling)
          }
        >
          Train
        </Button>
      </form>
    </section>
  );
}
