import { Button, Checkbox, Input, Select } from '@zone/ui';
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
  key: string;
  filename: string;
  bytes_base64: string;
  reading: boolean;
};

type Draft = {
  key: string;
  filename: string;
  caption: string;
  captionRevision: number;
  instruction: string;
  bytes_base64: string;
  reading: boolean;
  reference?: Reference;
  group?: number;
  source?: string;
  mirrored?: boolean;
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
  return !image.reference;
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
  document.getElementById(id)?.focus();
}

function readiness(images: Draft[]): string {
  const reading = images.filter((image) => image.reading || image.reference?.reading).length;
  if (reading > 0) {
    return `Reading selected ${reading === 1 ? 'image' : 'images'}.`;
  }
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

// Frames of one clip are all named alike, so the clip they came from is what
// tells them apart, and a mirrored one is worth saying so its caption can allow
// for it.
function captionOf(image: Draft): string {
  const named = image.source ? `${image.source} ${image.filename}` : image.filename;
  return image.mirrored ? `${named} (mirrored)` : named;
}

// Frames arrive grouped per clip, so a second clip has to be shifted past the
// groups already on the list or the two clips would be captioned as one.
function nextGroup(images: Draft[]): number {
  return images.reduce((highest, image) => Math.max(highest, (image.group ?? -1) + 1), 0);
}

export default function TrainPanel({ onTrained }: { onTrained: () => void }) {
  const [bases, setBases] = useState<TrainBase[]>([]);
  const [name, setName] = useState('');
  const [base, setBase] = useState('');
  const [trigger, setTrigger] = useState('');
  const [images, setImages] = useState<Draft[]>([]);
  const [mirror, setMirror] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<TrainResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [captioning, setCaptioning] = useState(false);
  const [focusRequested, setFocusRequested] = useState(false);
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

  useEffect(() => {
    if (!focusRequested) return;
    focusIncomplete(images);
    setFocusRequested(false);
  }, [focusRequested, images]);

  const selected = bases.find((row) => row.id === base);
  const edit = Boolean(selected?.edit);
  const pending = edit ? incomplete(images) : [];
  const reading = images.some((image) => image.reading || image.reference?.reading);
  const ready =
    Boolean(name.trim() && base && (edit || trigger.trim()) && images.length > 0 && !reading) &&
    pending.length === 0;

  const handleTargets = async (files: File[]) => {
    if (busy || files.length === 0) return;
    const added = files.map((file) => ({
      file,
      draft: {
        key: nextKey(),
        filename: file.name,
        caption: '',
        captionRevision: 0,
        instruction: '',
        bytes_base64: '',
        reading: true,
      } satisfies Draft,
    }));
    setImages((current) => [...current, ...added.map(({ draft }) => draft)]);
    if (edit) setFocusRequested(true);

    await Promise.all(
      added.map(async ({ draft, file }) => {
        try {
          const bytes_base64 = await fileToBase64(file);
          setImages((current) =>
            current.map((image) =>
              image.key === draft.key ? { ...image, bytes_base64, reading: false } : image
            )
          );
        } catch (caught) {
          setImages((current) => current.filter((image) => image.key !== draft.key));
          setError(caught instanceof Error ? caught.message : `Failed to read ${file.name}`);
        }
      })
    );
  };

  const handleReference = async (key: string, files: FileList | null) => {
    if (busy || files?.length !== 1) return;
    const [file] = Array.from(files);
    const reference: Reference = {
      key: nextKey(),
      filename: file.name,
      bytes_base64: '',
      reading: true,
    };
    setImages((current) =>
      current.map((image) => (image.key === key ? { ...image, reference } : image))
    );
    try {
      const bytes_base64 = await fileToBase64(file);
      setImages((current) =>
        current.map((image) =>
          image.key === key && image.reference?.key === reference.key
            ? { ...image, reference: { ...reference, bytes_base64, reading: false } }
            : image
        )
      );
    } catch (caught) {
      setImages((current) =>
        current.map((image) =>
          image.key === key && image.reference?.key === reference.key
            ? { ...image, reference: undefined }
            : image
        )
      );
      setError(caught instanceof Error ? caught.message : `Failed to read ${file.name}`);
    }
  };

  const handleBase = (value: string) => {
    if (busy) return;
    const nextEdit = Boolean(bases.find((row) => row.id === value)?.edit);
    setBase(value);
    if (!nextEdit) {
      setImages((current) =>
        current.map((image) => ({ ...image, instruction: '', reference: undefined }))
      );
      return;
    }
    if (!edit) setFocusRequested(true);
  };

  const handleVideos = async (files: FileList | null) => {
    if (busy || !files?.length) return;
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
              key: nextKey(),
              filename: frame.filename,
              caption: '',
              captionRevision: 0,
              instruction: '',
              bytes_base64: frame.bytes_base64,
              reading: false,
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
    if (busy || edit || reading || images.length === 0) return;
    const requested = images.map(
      ({ key, filename, caption, captionRevision, bytes_base64, group }) => ({
        key,
        filename,
        caption,
        captionRevision,
        bytes_base64,
        group,
      })
    );
    setCaptioning(true);
    setError(null);
    try {
      const { captions } = await modelsApi.captions({
        trigger: trigger.trim() || undefined,
        images: requested.map(({ filename, caption, bytes_base64, group }) => ({
          filename,
          caption,
          bytes_base64,
          group,
        })),
      });
      const generated = new Map(
        requested.map((image, index) => [
          image.key,
          { caption: captions[index], revision: image.captionRevision },
        ])
      );
      setImages((current) =>
        current.map((image) => {
          const result = generated.get(image.key);
          if (!result?.caption || image.captionRevision !== result.revision) return image;
          return { ...image, caption: result.caption };
        })
      );
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Captioning failed');
    } finally {
      setCaptioning(false);
    }
  };

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy || !ready) {
      if (!busy && edit) setFocusRequested(true);
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
          group: image.group,
          ...(edit && image.reference ? { before_base64: image.reference.bytes_base64 } : {}),
        })),
      });
      setResult(trained);
      setImages([]);
      setName('');
      setSampled(null);
      onTrained();
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Training failed');
    } finally {
      setBusy(false);
    }
  };

  const move = (key: string, direction: -1 | 1) => {
    if (busy) return;
    setImages((current) => {
      const index = current.findIndex((image) => image.key === key);
      const destination = index + direction;
      if (index < 0 || destination < 0 || destination >= current.length) return current;
      const next = [...current];
      [next[index], next[destination]] = [next[destination], next[index]];
      return next;
    });
  };

  return (
    <section className="card">
      <h2>Train a LoRA</h2>
      <p className="help-text">
        {edit
          ? 'Add target images, then pair each one with the reference image and instruction that produced it.'
          : 'Drop images or a video, pick an installed base, and set a unique trigger word. Every image is cropped square on its subject, then Zone trains every transformer block (rank 32, alpha equals rank, 400+ steps) so the LoRA can keep that identity.'}
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
      <form className="ui-form" aria-busy={busy} onSubmit={handleSubmit}>
        <Input
          label="Name"
          value={name}
          disabled={busy}
          onChange={(event) => {
            if (!busy) setName(event.target.value);
          }}
          required
        />
        <Select
          label="Base"
          value={base}
          onValueChange={handleBase}
          options={bases.map((row) => ({ value: row.id, label: row.label }))}
          placeholder="No trainable base installed"
          disabled={busy || bases.length === 0}
        />
        <Input
          label="Trigger word"
          value={trigger}
          disabled={busy}
          onChange={(event) => {
            if (!busy) setTrigger(event.target.value);
          }}
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
          disabled={busy}
          multiple
          onChange={(event) => {
            const files = Array.from(event.target.files ?? []);
            event.target.value = '';
            void handleTargets(files);
          }}
        />
        {!edit && (
          <>
            <Input
              label="Video"
              type="file"
              accept="video/*"
              multiple
              disabled={busy || Boolean(sampling)}
              onChange={(event) => void handleVideos(event.target.files)}
            />
            <p className="help-text">
              A clip is sampled above the rate it keeps, so the sharpest frame of each moment wins
              its slot, repeats of a shot already taken are dropped, and every frame is cropped
              around whatever moved.
            </p>
            <Checkbox
              label="Mirror half the frames of each second"
              helpText="More variety from one angle, applied as each clip is read. Turn it off for a subject carrying text, or one a mirror would get wrong."
              checked={mirror}
              disabled={busy}
              onCheckedChange={setMirror}
            />
            {sampling && <p className="help-text">Reading {sampling}…</p>}
            {sampled && <p className="help-text">{sampled}</p>}
          </>
        )}
        {images.length > 0 && !edit && (
          <div>
            <Button
              type="button"
              variant="secondary"
              loading={captioning}
              disabled={busy || captioning || reading}
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
              const named = captionOf(image);
              const referenceLabel = `Reference image for target ${number}: ${named}`;
              const instructionLabel = `Instruction for target ${number}: ${named}`;
              return (
                <fieldset
                  className="train-pair"
                  key={image.key}
                  aria-label={`Target pair ${number}: ${named}`}
                  aria-busy={image.reading || Boolean(image.reference?.reading)}
                >
                  <legend className="train-pair-title">
                    <span>Target {number}</span>
                    <span className="train-pair-filename">{named}</span>
                  </legend>
                  {edit && (
                    <>
                      <Input
                        id={`train-reference-${image.key}`}
                        label={referenceLabel}
                        type="file"
                        accept="image/png,image/jpeg,image/webp"
                        disabled={busy}
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
                    label={edit ? instructionLabel : `Caption for ${named}`}
                    value={edit ? image.instruction : image.caption}
                    disabled={busy}
                    required={edit}
                    error={
                      edit && missingInstruction(image)
                        ? 'Describe the edit that turns the reference into this target.'
                        : undefined
                    }
                    onChange={(event) => {
                      if (busy) return;
                      const value = event.target.value;
                      setImages((current) =>
                        current.map((currentImage) =>
                          currentImage.key === image.key
                            ? edit
                              ? { ...currentImage, instruction: value }
                              : {
                                  ...currentImage,
                                  caption: value,
                                  captionRevision: currentImage.captionRevision + 1,
                                }
                            : currentImage
                        )
                      );
                    }}
                  />
                  <div className="train-pair-actions" role="group" aria-label={`Arrange ${named}`}>
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      disabled={busy || index === 0}
                      aria-label={`Move target ${number}: ${named} up`}
                      onClick={() => move(image.key, -1)}
                    >
                      Move up
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      disabled={busy || index === images.length - 1}
                      aria-label={`Move target ${number}: ${named} down`}
                      onClick={() => move(image.key, 1)}
                    >
                      Move down
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      disabled={busy}
                      aria-label={`Remove target ${number}: ${named}`}
                      onClick={() => {
                        if (busy) return;
                        setImages((current) =>
                          current.filter((currentImage) => currentImage.key !== image.key)
                        );
                      }}
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
          disabled={busy || !ready || Boolean(sampling)}
          aria-describedby={edit ? 'train-pairs-status' : undefined}
        >
          Train
        </Button>
      </form>
    </section>
  );
}
