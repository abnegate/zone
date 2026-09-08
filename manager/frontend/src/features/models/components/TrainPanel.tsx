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

type Reference = {
  filename: string;
  bytes_base64: string;
};

type Draft = {
  key: string;
  filename: string;
  caption: string;
  instruction: string;
  bytes_base64: string;
  reference?: Reference;
};

type Band = 'none' | 'weak' | 'healthy' | 'strong';

const FLUX_CALIBRATION: TrainQuality['calibration'] = 'flux_health_bands';

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

let sequence = 0;

function nextKey(): string {
  sequence += 1;
  return `training-image-${sequence}`;
}

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

  if (quality.calibration !== FLUX_CALIBRATION) {
    return (
      <div className="train-quality">
        <div className="train-quality-head">
          <span className="tag train-band-uncalibrated">Measured, not calibrated</span>
          <span className="train-improvement">
            {Math.round(quality.improvement * 100)}% better than base
          </span>
          <span className="train-checkpoint">measured at {quality.checkpoint}</span>
        </div>
        <p className="train-quality-meaning">
          Qwen Image Edit health bands are not calibrated yet. This numeric comparison is not a
          weak, healthy, or strong rating and does not establish edit fidelity.
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

function missingReference(image: Draft): boolean {
  return !image.reference?.bytes_base64;
}

function missingInstruction(image: Draft): boolean {
  return image.instruction.trim().length === 0;
}

function incomplete(images: Draft[]): Draft[] {
  return images.filter((image) => missingReference(image) || missingInstruction(image));
}

function focusIncomplete(images: Draft[]): void {
  const image = incomplete(images)[0];
  if (!image) return;
  const id = missingReference(image)
    ? `train-reference-${image.key}`
    : `train-instruction-${image.key}`;
  setTimeout(() => document.getElementById(id)?.focus(), 0);
}

function readiness(images: Draft[]): string {
  const pending = incomplete(images);
  if (images.length === 0) return 'Add at least one target image to begin pairing.';
  if (pending.length === 0) return 'Every target has one reference image and one instruction.';

  const references = pending.filter(missingReference).length;
  const instructions = pending.filter(missingInstruction).length;
  let missing = 'a reference image and instruction';
  if (references === 0) missing = 'an instruction';
  if (instructions === 0) missing = 'a reference image';
  return `${pending.length} target ${pending.length === 1 ? 'pair still needs' : 'pairs still need'} ${missing}.`;
}

export default function TrainPanel({ onTrained }: { onTrained: () => void }) {
  const [bases, setBases] = useState<TrainBase[]>([]);
  const [name, setName] = useState('');
  const [base, setBase] = useState('');
  const [trigger, setTrigger] = useState('');
  const [images, setImages] = useState<Draft[]>([]);
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
  const pending = edit ? incomplete(images) : [];
  const ready =
    Boolean(name.trim() && base && (edit || trigger.trim()) && images.length > 0) &&
    pending.length === 0;

  const handleTargets = async (files: File[]) => {
    if (files.length === 0) return;
    const added: Draft[] = [];
    for (const file of files) {
      added.push({
        key: nextKey(),
        filename: file.name,
        caption: '',
        instruction: '',
        bytes_base64: await fileToBase64(file),
      });
    }
    const next = [...images, ...added];
    setImages(next);
    if (edit) focusIncomplete(next);
  };

  const handleReference = async (key: string, files: FileList | null) => {
    if (files?.length !== 1) return;
    const [file] = Array.from(files);
    const reference = { filename: file.name, bytes_base64: await fileToBase64(file) };
    setImages((current) =>
      current.map((image) => (image.key === key ? { ...image, reference } : image))
    );
  };

  const handleBase = (value: string) => {
    const nextEdit = Boolean(bases.find((row) => row.id === value)?.edit);
    setBase(value);
    if (!nextEdit) {
      setImages((current) =>
        current.map((image) => ({ ...image, instruction: '', reference: undefined }))
      );
      return;
    }
    if (!edit) focusIncomplete(images);
  };

  const handleCaption = async () => {
    if (edit || images.length === 0) return;
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
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Captioning failed');
    } finally {
      setCaptioning(false);
    }
  };

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (!ready) {
      if (edit) focusIncomplete(images);
      return;
    }
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      const trained = await modelsApi.train({
        name: name.trim(),
        base,
        trigger: trigger.trim() || undefined,
        images: images.map((image) => ({
          filename: image.filename,
          caption: edit ? image.instruction.trim() : image.caption,
          bytes_base64: image.bytes_base64,
          ...(edit && image.reference ? { before_base64: image.reference.bytes_base64 } : {}),
        })),
      });
      setResult(trained);
      setImages([]);
      setName('');
      onTrained();
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Training failed');
    } finally {
      setBusy(false);
    }
  };

  const move = (index: number, direction: -1 | 1) => {
    const destination = index + direction;
    if (destination < 0 || destination >= images.length) return;
    const next = [...images];
    [next[index], next[destination]] = [next[destination], next[index]];
    setImages(next);
  };

  return (
    <section className="card">
      <h2>Train a LoRA</h2>
      <p className="help-text">
        {edit
          ? 'Add target images, then pair each one with the reference image and instruction that produced it.'
          : 'Drop images, pick an installed base, and set a unique trigger word. Zone trains every transformer block (rank 32, alpha equals rank, 400+ steps).'}
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
          onValueChange={handleBase}
          options={bases.map((row) => ({ value: row.id, label: row.label }))}
          placeholder="No trainable base installed"
          disabled={bases.length === 0}
        />
        <Input
          label="Trigger word"
          value={trigger}
          onChange={(event) => setTrigger(event.target.value)}
          placeholder={edit ? 'optional subject name' : 'required for a unique identity'}
          required={!edit}
        />
        <Input
          id="train-targets"
          label="Target images"
          helpText={
            edit
              ? 'Choose the finished images. You will add one reference and instruction for each target.'
              : 'Choose the images this LoRA should learn from.'
          }
          type="file"
          accept="image/png,image/jpeg,image/webp"
          multiple
          onChange={(event) => {
            const files = Array.from(event.target.files ?? []);
            event.target.value = '';
            void handleTargets(files);
          }}
        />
        {images.length > 0 && !edit && (
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
        {edit && (
          <p
            id="train-pairs-status"
            className="train-pairs-status"
            role="status"
            aria-label="Training pair readiness"
            aria-live="polite"
          >
            {readiness(images)}
          </p>
        )}
        {images.length > 0 && (
          <div className="train-pairs">
            {images.map((image, index) => {
              const number = index + 1;
              const referenceLabel = `Reference image for target ${number}: ${image.filename}`;
              const instructionLabel = `Instruction for target ${number}: ${image.filename}`;
              return (
                <fieldset
                  className="train-pair"
                  key={image.key}
                  aria-label={`Target pair ${number}: ${image.filename}`}
                >
                  <legend className="train-pair-title">
                    <span>Target {number}</span>
                    <span className="train-pair-filename">{image.filename}</span>
                  </legend>
                  {edit && (
                    <>
                      <Input
                        id={`train-reference-${image.key}`}
                        label={referenceLabel}
                        type="file"
                        accept="image/png,image/jpeg,image/webp"
                        aria-required="true"
                        error={
                          missingReference(image)
                            ? 'Choose one reference image for this target.'
                            : undefined
                        }
                        onChange={(event) => {
                          const files = event.target.files;
                          void handleReference(image.key, files);
                        }}
                      />
                      {image.reference && (
                        <p className="train-reference-name">
                          Reference: {image.reference.filename}
                        </p>
                      )}
                    </>
                  )}
                  <Input
                    id={edit ? `train-instruction-${image.key}` : `train-caption-${image.key}`}
                    label={edit ? instructionLabel : `Caption for ${image.filename}`}
                    value={edit ? image.instruction : image.caption}
                    required={edit}
                    error={
                      edit && missingInstruction(image)
                        ? 'Describe the edit that turns the reference into this target.'
                        : undefined
                    }
                    onChange={(event) => {
                      const value = event.target.value;
                      setImages((current) =>
                        current.map((currentImage) =>
                          currentImage.key === image.key
                            ? edit
                              ? { ...currentImage, instruction: value }
                              : { ...currentImage, caption: value }
                            : currentImage
                        )
                      );
                    }}
                  />
                  <div
                    className="train-pair-actions"
                    role="group"
                    aria-label={`Arrange ${image.filename}`}
                  >
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      disabled={index === 0}
                      aria-label={`Move target ${number}: ${image.filename} up`}
                      onClick={() => move(index, -1)}
                    >
                      Move up
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      disabled={index === images.length - 1}
                      aria-label={`Move target ${number}: ${image.filename} down`}
                      onClick={() => move(index, 1)}
                    >
                      Move down
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      aria-label={`Remove target ${number}: ${image.filename}`}
                      onClick={() =>
                        setImages((current) =>
                          current.filter((currentImage) => currentImage.key !== image.key)
                        )
                      }
                    >
                      Remove
                    </Button>
                  </div>
                </fieldset>
              );
            })}
          </div>
        )}
        <Button
          type="submit"
          loading={busy}
          disabled={!ready}
          aria-describedby={edit ? 'train-pairs-status' : undefined}
        >
          Train
        </Button>
      </form>
    </section>
  );
}
