import { Button, Input, Select } from '@zone/ui';
import { type FormEvent, useEffect, useState } from 'react';
import { modelsApi } from '../../../api/models';

type TrainBase = { id: string; label: string; edit: boolean };

type TrainImage = {
  filename: string;
  caption: string;
  bytes_base64: string;
  before_base64?: string;
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

export default function TrainPanel({ onTrained }: { onTrained: () => void }) {
  const [bases, setBases] = useState<TrainBase[]>([]);
  const [name, setName] = useState('');
  const [base, setBase] = useState('');
  const [trigger, setTrigger] = useState('');
  const [images, setImages] = useState<TrainImage[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [captioning, setCaptioning] = useState(false);

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
    const next = [...images];
    for (const file of Array.from(files)) {
      const encoded = await fileToBase64(file);
      if (before) {
        const index = next.findIndex((image) => !image.before_base64);
        if (index >= 0) {
          next[index] = { ...next[index], before_base64: encoded };
        }
      } else {
        next.push({ filename: file.name, caption: '', bytes_base64: encoded });
      }
    }
    setImages(next);
  };

  const handleCaption = async () => {
    if (images.length === 0) return;
    setCaptioning(true);
    setError(null);
    try {
      const { captions } = await modelsApi.captions({
        trigger: trigger.trim() || undefined,
        images: images.map(({ filename, caption, bytes_base64 }) => ({
          filename,
          caption,
          bytes_base64,
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
        images,
      });
      setImages([]);
      setName('');
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
        Drop images, pick an installed base, and set a unique trigger word. Zone trains every
        transformer block (rank 8, alpha equals rank, 400+ steps) so the LoRA can keep that
        identity.
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
              Captions you have written are kept.
            </p>
          </div>
        )}
        {images.map((image, index) => (
          <Input
            key={`${image.filename}-${index}`}
            id={`train-caption-${index}`}
            label={`Caption for ${image.filename}`}
            value={image.caption}
            onChange={(event) => {
              const next = [...images];
              next[index] = { ...image, caption: event.target.value };
              setImages(next);
            }}
          />
        ))}
        <Button
          type="submit"
          loading={busy}
          disabled={!name.trim() || !base || !trigger.trim() || images.length === 0}
        >
          Train
        </Button>
      </form>
    </section>
  );
}
