import { Button, Input, Select } from '@zone/ui';
import { type FormEvent, type ReactElement, useEffect, useState } from 'react';
import {
  type DatasetConcern,
  type DatasetFinding,
  type DropReason,
  modelsApi,
  type TrainQuality,
  type TrainResult,
  type TrainScreening,
} from '../../../api/models';
import './TrainPanel.css';

type TrainBase = { id: string; label: string; edit: boolean };

type TrainImage = {
  filename: string;
  caption: string;
  bytes_base64: string;
  before_base64?: string;
};

type Band = 'none' | 'weak' | 'healthy' | 'strong';

const BANDS: Record<Band, { label: string; meaning: string }> = {
  none: {
    label: 'No measurable learning',
    meaning:
      'An adapter that learned nothing still scores around 15%, so this run cannot be told apart from one. Train again with more images or a longer run.',
  },
  weak: {
    label: 'Weak',
    meaning:
      'Clear of the 15% a run that learned nothing scores, but short of the 35% a healthy run reaches.',
  },
  healthy: {
    label: 'Healthy',
    meaning: 'The range a good short run reaches, around 35%.',
  },
  strong: {
    label: 'Strong',
    meaning: 'As high as a full-length run reaches, around 46%.',
  },
};

const CONCERNS: Record<DatasetConcern, string> = {
  too_few: 'Too few images',
  low_variety: 'Images too alike',
  low_pose_variety: 'Cannot be prompted into new poses',
  mixed_subjects: 'More than one subject',
};

const REASONS: Record<DropReason, { singular: string; plural: string; meaning: string }> = {
  duplicate: {
    singular: 'near-duplicate',
    plural: 'near-duplicates',
    meaning: 'Near-duplicate frames teach one pose over and over.',
  },
  blurred: {
    singular: 'blurred frame',
    plural: 'blurred frames',
    meaning: 'Motion blur is learned as part of the subject.',
  },
  small: {
    singular: 'undersized image',
    plural: 'undersized images',
    meaning: 'Below training resolution there is no detail left to learn.',
  },
};

function band(improvement: number): Band {
  if (improvement <= 0.15) return 'none';
  if (improvement < 0.3) return 'weak';
  if (improvement < 0.45) return 'healthy';
  return 'strong';
}

function Quality({ quality }: { quality: TrainQuality | null }): ReactElement {
  if (!quality?.measured || !Number.isFinite(quality.improvement)) {
    return (
      <div className="train-quality">
        <div className="train-quality-head">
          <span className="tag">Not measured</span>
        </div>
        <p className="train-quality-meaning">
          Quality probing is best effort and did not run for this LoRA. The adapter trained
          normally, there is simply no score to show for it.
        </p>
      </div>
    );
  }

  const level = band(quality.improvement);
  return (
    <div className="train-quality">
      <div className="train-quality-head">
        <span className={`tag train-band-${level}`}>{BANDS[level].label}</span>
        <span className="train-improvement">{Math.round(quality.improvement * 100)}% better</span>
        <span className="train-checkpoint">measured at {quality.checkpoint}</span>
      </div>
      <p className="train-quality-meaning">{BANDS[level].meaning}</p>
    </div>
  );
}

function label(reason: DropReason, count: number): string {
  const { singular, plural } = REASONS[reason];
  return `${count} ${count === 1 ? singular : plural}`;
}

function Screening({ screening }: { screening: TrainScreening | null }): ReactElement | null {
  if (!screening?.dropped?.length) return null;

  const groups = (Object.keys(REASONS) as DropReason[])
    .map((reason) => ({
      reason,
      filenames: screening.dropped
        .filter((image) => image.reason === reason)
        .map((image) => image.filename),
    }))
    .filter((group) => group.filenames.length > 0);

  if (groups.length === 0) return null;

  return (
    <div className="train-screening">
      <p className="train-screening-title">Screened before training</p>
      <p className="train-screening-summary">
        Trained on {screening.kept} of {screening.kept + screening.dropped.length} images. These
        were set aside:
      </p>
      <ul className="train-screening-list">
        {groups.map((group) => (
          <li className="train-screening-item" key={group.reason}>
            <span className="tag train-drop">{label(group.reason, group.filenames.length)}</span>
            <span>{REASONS[group.reason].meaning}</span>
            <span className="train-screening-files">{group.filenames.join(', ')}</span>
          </li>
        ))}
      </ul>
      <p className="train-screening-note">
        Your originals are untouched. Screening only decides what a run learns from.
      </p>
    </div>
  );
}

function Advice({ findings }: { findings: DatasetFinding[] }): ReactElement | null {
  if (findings.length === 0) return null;

  return (
    <div className="train-advice">
      <p className="train-advice-title">Worth checking</p>
      <ul className="train-advice-list">
        {findings.map((finding) => (
          <li className="train-advice-item" key={`${finding.concern}:${finding.detail}`}>
            {CONCERNS[finding.concern] && <span className="tag">{CONCERNS[finding.concern]}</span>}
            <span>{finding.detail}</span>
          </li>
        ))}
      </ul>
      <p className="train-advice-caveat">
        Advice only, from a quick look at your images. It is a rough check and can be wrong.
      </p>
    </div>
  );
}

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
  const [result, setResult] = useState<TrainResult | null>(null);
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
    setResult(null);
    try {
      const trained = await modelsApi.train({
        name: name.trim(),
        base,
        trigger: trigger.trim() || undefined,
        images,
      });
      setResult(trained);
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
        transformer block (rank 32, alpha equals rank, 400+ steps) so the LoRA can keep that
        identity.
      </p>
      {error && <div className="error-placeholder">{error}</div>}
      {result && (
        <div className="train-result" role="status">
          <h3 className="train-result-title">
            Training finished{result.filename ? `: ${result.filename}` : ''}
          </h3>
          <Quality quality={result.quality} />
          <Screening screening={result.screening ?? null} />
          <Advice findings={result.dataset ?? []} />
        </div>
      )}
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
