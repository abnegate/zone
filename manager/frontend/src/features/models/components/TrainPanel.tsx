import { Button, Checkbox, Input, Select } from '@zone/ui';
import { type FormEvent, type ReactElement, useEffect, useMemo, useState } from 'react';
import { List, type RowComponentProps } from 'react-window';
import {
  type DatasetConcern,
  type DatasetFinding,
  type DropReason,
  modelsApi,
  type TrainJob,
  type TrainQuality,
  type TrainRemediation,
  type TrainResult,
  type TrainScreening,
} from '../../../api/models';
import { isImageFile, isVideoFile } from '../dropFiles';
import {
  blobFromBase64,
  captionBatches,
  FRAME_UPLOADS,
  PAIR_LIST_MAX,
  PAIR_ROW_HEIGHT,
  poolMap,
  prepareImage,
} from '../trainMedia';
import { methodAdvice, type TrainMethodKind } from '../utils/trainProgress';
import DropZone from './DropZone';
import TrainMeter from './TrainMeter';
import './TrainPanel.css';

type TrainBase = { id: string; label: string; edit: boolean };
type TrainSubjectKind = 'person' | 'other';

const PEOPLE_BASE = 'sdxl-people';
const PEOPLE_BUNDLE_HELP =
  'The SDXL people bundle must be downloaded before training a person: ./scripts/setup-comfyui-macos.sh --download-model --bundle image-people';

function firstOtherBase(bases: TrainBase[]): string {
  return bases.find((row) => row.id !== PEOPLE_BASE)?.id ?? '';
}

function trainingHeading(method: TrainMethodKind): string {
  if (method === 'finetune') return 'Fine-tune a person';
  if (method === 'pivotal') return 'Pivotal training';
  if (method === 'video') return 'Train video identity';
  return 'Train a LoRA';
}

function trainingHelp(subject: TrainSubjectKind, method: TrainMethodKind, edit: boolean): string {
  if (subject === 'person' && method === 'finetune') {
    return 'Drop images, clips, or a folder, set a unique trigger word, and fine-tune the SDXL people checkpoint. A run takes days to weeks, writes a ~7 GB checkpoint (full UNet + CLIP-L, prior preservation), and resumes after a refresh or restart. Plan ~20 GB of disk during a run. Fine-tune from about 200 unique stills or 20 clips; 500 stills or 50 clips is the strong set for likeness, hands, and body.';
  }
  if (subject === 'person' && method === 'pivotal') {
    return 'Drop images, clips, or a folder, set a unique trigger word, and train a rank-64 SDXL people LoRA plus a CLIP-L textual inversion of the trigger. Hours, same dump as LoRA, cleaner promptability. Face and body still come from the people bundle.';
  }
  if (subject === 'person' && method === 'video') {
    return 'Drop clips (stills in the dump are ignored for this method), set a unique trigger word, and train a Wan 2.2 TI2V 5B video LoRA from 2–3 windows per clip. Run a still method too for image identity. Needs the Wan video bundle on the host.';
  }
  if (subject === 'person') {
    return 'Drop images, clips, or a folder, set a unique trigger word, and train an SDXL people adapter. A run takes hours and writes a ~100–200 MB adapter (1024 buckets, rank 64, text encoder trained). Body shots stay in frame; a tighter head crop is added so faces stay sharp. LoRA fits about 20–200 unique stills. Fine-tune starts around 200 stills or 20 clips; 500 stills or 50 clips is the strong set.';
  }
  if (edit) {
    return 'Add target images, then pair each one with the reference image and instruction that produced it.';
  }
  return 'Drop images, clips, or a folder, pick an installed base, and set a unique trigger word. Every image is cropped square on its subject, then Zone trains every transformer block (rank 32, alpha equals rank, 400+ steps) so the LoRA can keep that identity.';
}

type Reference = {
  key: string;
  filename: string;
  blob: Blob;
};

type Draft = {
  key: string;
  filename: string;
  caption: string;
  captionRevision: number;
  instruction: string;
  blob: Blob;
  reference?: Reference;
  group?: number;
  source?: string;
  clip?: string;
  mirrored?: boolean;
};

const IMAGE_ACCEPT = 'image/png,image/jpeg,image/webp';
const MIXED_ACCEPT = `${IMAGE_ACCEPT},video/*`;

type Clip = { key: string; name: string; summary: string };

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

const REMEDIATION_OUTCOMES: Record<TrainRemediation['outcome'], string> = {
  used: 'It passed the final screen, and the improved copy was used.',
  still_rejected: 'It did not pass the final screen, so it was not used.',
  failed: 'Zone could not create an improved copy, so it was not used.',
};

let sequence = 0;

function nextKey(prefix = 'training-image'): string {
  sequence += 1;
  return `${prefix}-${sequence}`;
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
  if (!screening) return null;

  const groups = (Object.keys(REASONS) as DropReason[])
    .map((reason) => ({
      reason,
      filenames: screening.dropped
        .filter((image) => image.reason === reason)
        .map((image) => image.filename),
    }))
    .filter((group) => group.filenames.length > 0);
  const attempted = screening.attempted ?? [];

  if (groups.length === 0 && attempted.length === 0) return null;

  return (
    <div className="train-screening">
      <p className="train-screening-title">Screened before training</p>
      {groups.length > 0 && (
        <>
          <p className="train-screening-summary">
            Trained on {screening.kept} of {screening.kept + screening.dropped.length} images. These
            were set aside:
          </p>
          <ul className="train-screening-list">
            {groups.map((group) => (
              <li className="train-screening-item" key={group.reason}>
                <span className="tag train-drop">
                  {label(group.reason, group.filenames.length)}
                </span>
                <span>{REASONS[group.reason].meaning}</span>
                <span className="train-screening-files">{group.filenames.join(', ')}</span>
              </li>
            ))}
          </ul>
        </>
      )}
      {attempted.length > 0 && (
        <>
          <p className="train-screening-title">Image repairs</p>
          <p className="train-screening-summary">
            Zone tried to improve these images before the final screen:
          </p>
          <ul className="train-screening-list">
            {attempted.map((image) => (
              <li className="train-screening-item" key={image.source_index}>
                <span className="tag train-drop">{image.filename}</span>
                <span>
                  We tried to improve this {REASONS[image.reason].singular}.{' '}
                  {REMEDIATION_OUTCOMES[image.outcome]}
                </span>
              </li>
            ))}
          </ul>
        </>
      )}
      <p className="train-screening-note">
        Your originals are untouched. Zone only uses repaired copies inside this training run.
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

function Thumbnail({ blob }: { blob: Blob }): ReactElement {
  const [source, setSource] = useState<string | null>(null);
  useEffect(() => {
    const url = URL.createObjectURL(blob);
    setSource(url);
    return () => {
      URL.revokeObjectURL(url);
    };
  }, [blob]);
  return (
    <div className="train-pair-thumb" aria-hidden="true">
      {source && <img src={source} alt="" />}
    </div>
  );
}

function ArrowIcon({ direction }: { direction: 'up' | 'down' }): ReactElement {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {direction === 'up' ? (
        <path d="M12 19V5M5 12l7-7 7 7" />
      ) : (
        <path d="M12 5v14M19 12l-7 7-7-7" />
      )}
    </svg>
  );
}

function CloseIcon(): ReactElement {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M18 6L6 18M6 6l12 12" />
    </svg>
  );
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

type PairRowProps = {
  images: Draft[];
  edit: boolean;
  busy: boolean;
  onCaption: (key: string, value: string) => void;
  onInstruction: (key: string, value: string) => void;
  onReference: (key: string, files: FileList | null) => void;
  onMove: (key: string, direction: -1 | 1) => void;
  onRemove: (key: string) => void;
};

function TrainPairRow({
  index,
  style,
  images,
  edit,
  busy,
  onCaption,
  onInstruction,
  onReference,
  onMove,
  onRemove,
}: RowComponentProps<PairRowProps>): ReactElement {
  const image = images[index];
  const number = index + 1;
  const named = captionOf(image);
  const referenceLabel = `Reference image for target ${number}: ${named}`;
  const instructionLabel = `Instruction for target ${number}: ${named}`;
  return (
    <div style={style}>
      <fieldset
        className="train-pair"
        aria-label={`Target pair ${number}: ${named}`}
        aria-busy="false"
      >
        <Thumbnail blob={image.blob} />
        <div className="train-pair-main">
          <div className="train-pair-head">
            <span className="train-pair-index">Target {number}</span>
            <span className="train-pair-filename">{named}</span>
          </div>
          <div className={`train-pair-fields ${edit ? 'train-pair-fields--edit' : ''}`}>
            {edit && (
              <Input
                id={`train-reference-${image.key}`}
                aria-label={referenceLabel}
                type="file"
                accept={IMAGE_ACCEPT}
                disabled={busy}
                aria-required="true"
                helpText={image.reference ? `Reference: ${image.reference.filename}` : undefined}
                error={
                  missingReference(image)
                    ? 'Choose one reference image for this target.'
                    : undefined
                }
                onChange={(event) => {
                  onReference(image.key, event.target.files);
                }}
              />
            )}
            <Input
              id={edit ? `train-instruction-${image.key}` : `train-caption-${image.key}`}
              aria-label={edit ? instructionLabel : `Caption for ${named}`}
              placeholder={edit ? 'Describe the edit' : 'Caption'}
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
                if (edit) onInstruction(image.key, value);
                else onCaption(image.key, value);
              }}
            />
          </div>
        </div>
        <div className="train-pair-actions" role="group" aria-label={`Arrange ${named}`}>
          <Button
            type="button"
            size="icon"
            variant="ghost"
            disabled={busy || index === 0}
            aria-label={`Move target ${number}: ${named} up`}
            onClick={() => onMove(image.key, -1)}
          >
            <ArrowIcon direction="up" />
          </Button>
          <Button
            type="button"
            size="icon"
            variant="ghost"
            disabled={busy || index === images.length - 1}
            aria-label={`Move target ${number}: ${named} down`}
            onClick={() => onMove(image.key, 1)}
          >
            <ArrowIcon direction="down" />
          </Button>
          <Button
            type="button"
            size="icon"
            variant="ghost"
            disabled={busy}
            aria-label={`Remove target ${number}: ${named}`}
            onClick={() => onRemove(image.key)}
          >
            <CloseIcon />
          </Button>
        </div>
      </fieldset>
    </div>
  );
}

export default function TrainPanel({ onTrained }: { onTrained: () => void }) {
  const [bases, setBases] = useState<TrainBase[]>([]);
  const [subject, setSubject] = useState<TrainSubjectKind>('other');
  const [method, setMethod] = useState<TrainMethodKind>('lora');
  const [name, setName] = useState('');
  const [base, setBase] = useState('');
  const [trigger, setTrigger] = useState('');
  const [images, setImages] = useState<Draft[]>([]);
  const [mirror, setMirror] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<TrainResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [runName, setRunName] = useState<string | null>(null);
  const [progress, setProgress] = useState<TrainJob | null>(null);
  const [captioning, setCaptioning] = useState(false);
  const [focusRequested, setFocusRequested] = useState(false);
  const [sampling, setSampling] = useState<Array<{ key: string; name: string }>>([]);
  const [clips, setClips] = useState<Clip[]>([]);

  useEffect(() => {
    modelsApi
      .trainBases()
      .then(setBases)
      .catch(() => setBases([]));
  }, []);

  useEffect(() => {
    if (subject === 'person') {
      setBase(bases.some((row) => row.id === PEOPLE_BASE) ? PEOPLE_BASE : '');
      return;
    }
    setBase((current) => {
      if (current && current !== PEOPLE_BASE && bases.some((row) => row.id === current)) {
        return current;
      }
      return firstOtherBase(bases);
    });
  }, [bases, subject]);

  // Resume the process-wide job once. onTrained is the inventory refresh from
  // first mount; re-running this on a new callback would abort a live poll.
  // biome-ignore lint/correctness/useExhaustiveDependencies: resume once per mount
  useEffect(() => {
    const controller = new AbortController();
    const aborted = (caught: unknown) =>
      (caught instanceof DOMException && caught.name === 'AbortError') ||
      (caught instanceof Error && caught.name === 'AbortError');
    void (async () => {
      try {
        const job = await modelsApi.trainJob(controller.signal);
        if (controller.signal.aborted || !job) return;
        if (job.status === 'running') {
          setBusy(true);
          setRunName(job.name ?? null);
          setProgress(job);
          const trained = await modelsApi.waitTrain(controller.signal, setProgress);
          if (controller.signal.aborted) return;
          setResult(trained);
          setProgress(null);
          setImages([]);
          setClips([]);
          onTrained();
        } else if (job.status === 'succeeded') {
          setResult({
            filename: job.filename ?? null,
            quality: job.quality ?? null,
            dataset: job.dataset,
            screening: job.screening ?? null,
          });
        } else if (job.status === 'failed' && job.error) {
          setError(job.error);
        }
      } catch (caught) {
        if (!aborted(caught) && !controller.signal.aborted) {
          setError(caught instanceof Error ? caught.message : 'Could not read training status');
        }
      } finally {
        if (!controller.signal.aborted) setBusy(false);
      }
    })();
    return () => controller.abort();
  }, []);

  useEffect(() => {
    if (!focusRequested) return;
    focusIncomplete(images);
    setFocusRequested(false);
  }, [focusRequested, images]);

  const visibleBases =
    subject === 'person'
      ? bases.filter((row) => row.id === PEOPLE_BASE)
      : bases.filter((row) => row.id !== PEOPLE_BASE);
  const peopleMissing = subject === 'person' && visibleBases.length === 0;
  const selected = visibleBases.find((row) => row.id === base);
  const edit = subject === 'other' && Boolean(selected?.edit);
  const pending = edit ? incomplete(images) : [];
  const ready =
    Boolean(name.trim() && base && (edit || trigger.trim()) && images.length > 0) &&
    pending.length === 0 &&
    !peopleMissing;

  const handleTargets = (files: File[]) => {
    if (busy || files.length === 0) return;
    const added = files.filter(isImageFile).map(
      (file) =>
        ({
          key: nextKey(),
          filename: file.name,
          caption: '',
          captionRevision: 0,
          instruction: '',
          blob: file,
        }) satisfies Draft
    );
    if (added.length === 0) return;
    setImages((current) => [...current, ...added]);
    if (edit) setFocusRequested(true);
  };

  const handleReference = (key: string, files: FileList | null) => {
    if (busy || files?.length !== 1) return;
    const [file] = Array.from(files);
    const reference: Reference = {
      key: nextKey(),
      filename: file.name,
      blob: file,
    };
    setImages((current) =>
      current.map((image) => (image.key === key ? { ...image, reference } : image))
    );
  };

  const handleSubject = (value: string) => {
    if (busy) return;
    if (value === 'person') {
      setSubject('person');
      setBase(bases.some((row) => row.id === PEOPLE_BASE) ? PEOPLE_BASE : '');
      if (edit) {
        setImages((current) =>
          current.map((image) => ({ ...image, instruction: '', reference: undefined }))
        );
      }
      return;
    }
    setSubject('other');
    setMethod('lora');
    setBase(firstOtherBase(bases));
  };

  const handleMethod = (value: string) => {
    if (busy) return;
    if (value === 'finetune' || value === 'pivotal' || value === 'video') {
      if (subject !== 'person') return;
      setMethod(value);
      return;
    }
    setMethod('lora');
  };

  const handleBase = (value: string) => {
    if (busy) return;
    if (subject === 'person') {
      if (value !== PEOPLE_BASE) return;
      setBase(PEOPLE_BASE);
      return;
    }
    const nextEdit = Boolean(visibleBases.find((row) => row.id === value)?.edit);
    setBase(value);
    if (!nextEdit) {
      setImages((current) =>
        current.map((image) => ({ ...image, instruction: '', reference: undefined }))
      );
      return;
    }
    if (!edit) setFocusRequested(true);
  };

  const handleVideos = async (files: File[]) => {
    const clipsToRead = files.filter(isVideoFile);
    if (busy || clipsToRead.length === 0) return;
    setError(null);
    const jobs = clipsToRead.map((file) => ({ file, key: nextKey('training-clip') }));
    setSampling((current) => [
      ...current,
      ...jobs.map(({ key, file }) => ({ key, name: file.name })),
    ]);
    await poolMap(jobs, FRAME_UPLOADS, async ({ file, key }) => {
      try {
        const clip = await modelsApi.frames({
          filename: file.name,
          blob: file,
          mirror,
        });
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
              blob: blobFromBase64(frame.bytes_base64),
              group: offset + frame.group,
              source: file.name,
              clip: key,
              mirrored: frame.mirrored,
            })),
          ];
        });
        setClips((current) => [
          ...current,
          {
            key,
            name: file.name,
            summary: `${clip.sampled} frames read at ${clip.sampled_fps.toFixed(1)}/s, ${clip.frames.length} kept`,
          },
        ]);
      } catch (err) {
        setError(err instanceof Error ? err.message : `Could not read ${file.name}`);
      } finally {
        setSampling((current) => current.filter((item) => item.key !== key));
      }
    });
  };

  const handleDrop = (files: File[]) => {
    handleTargets(files);
    if (!edit) void handleVideos(files);
  };

  const removeClip = (key: string) => {
    if (busy) return;
    setClips((current) => current.filter((clip) => clip.key !== key));
    setImages((current) => current.filter((image) => image.clip !== key));
  };

  const handleCaption = async () => {
    if (busy || edit || images.length === 0) return;
    const requested = images.map(({ key, filename, caption, captionRevision, blob, group }) => ({
      key,
      filename,
      caption,
      captionRevision,
      blob,
      group,
    }));
    setCaptioning(true);
    setError(null);
    try {
      const generated = new Map<string, { caption: string; revision: number }>();
      for (const batch of captionBatches(requested)) {
        const prepared = await Promise.all(
          batch.map(async (image) => ({
            ...image,
            blob: await prepareImage(image.blob, image.filename),
          }))
        );
        const { captions } = await modelsApi.captions({
          trigger: trigger.trim() || undefined,
          images: prepared.map(({ filename, caption, blob, group }) => ({
            filename,
            caption,
            blob,
            group,
          })),
        });
        prepared.forEach((image, index) => {
          generated.set(image.key, {
            caption: captions[index],
            revision: image.captionRevision,
          });
        });
      }
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
    setRunName(name.trim() || null);
    setError(null);
    setResult(null);
    setProgress({ name: name.trim(), status: 'running' });
    try {
      const trained = await modelsApi.train(
        {
          name: name.trim(),
          base,
          trigger: trigger.trim() || undefined,
          subject,
          method,
          images: await Promise.all(
            images.map(async (image) => ({
              filename: image.filename,
              caption: edit ? image.instruction.trim() : image.caption,
              blob: await prepareImage(image.blob, image.filename),
              group: image.group,
              before:
                edit && image.reference
                  ? await prepareImage(image.reference.blob, image.reference.filename)
                  : undefined,
            }))
          ),
        },
        undefined,
        setProgress
      );
      setResult(trained);
      setProgress(null);
      setImages([]);
      setClips([]);
      setName('');
      onTrained();
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Training failed');
      setProgress(null);
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

  // biome-ignore lint/correctness/useExhaustiveDependencies: handlers close over setState
  const pairRowProps = useMemo<PairRowProps>(
    () => ({
      images,
      edit,
      busy,
      onCaption: (key, value) => {
        setImages((current) =>
          current.map((image) =>
            image.key === key
              ? { ...image, caption: value, captionRevision: image.captionRevision + 1 }
              : image
          )
        );
      },
      onInstruction: (key, value) => {
        setImages((current) =>
          current.map((image) => (image.key === key ? { ...image, instruction: value } : image))
        );
      },
      onReference: handleReference,
      onMove: move,
      onRemove: (key) => {
        if (busy) return;
        setImages((current) => current.filter((image) => image.key !== key));
      },
    }),
    [images, edit, busy]
  );

  const listHeight = Math.max(
    PAIR_ROW_HEIGHT,
    Math.min(images.length * PAIR_ROW_HEIGHT, PAIR_LIST_MAX)
  );
  const advice = methodAdvice({
    subject,
    method,
    stills: images.length,
    clips: clips.length,
  });

  return (
    <section className="card">
      <h2>{trainingHeading(method)}</h2>
      <p className="help-text">{trainingHelp(subject, method, edit)}</p>
      {advice && <p className="help-text">{advice}</p>}
      {error && <div className="error-placeholder">{error}</div>}
      {busy && (
        <div className="train-result" role="status">
          <h3 className="train-result-title">Training{runName ? ` ${runName}` : ''}</h3>
          <TrainMeter
            job={progress ?? { name: runName ?? undefined, status: 'running' }}
            detail="This run keeps going if you leave the page."
          />
        </div>
      )}
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
      <form className="ui-form train-form" aria-busy={busy} onSubmit={handleSubmit}>
        <div className="train-kind">
          <Select
            label="Subject"
            value={subject}
            onValueChange={handleSubject}
            options={[
              { value: 'person', label: 'Person' },
              { value: 'other', label: 'Other' },
            ]}
            disabled={busy}
          />
          <Select
            label="Method"
            value={method}
            onValueChange={handleMethod}
            options={[
              { value: 'lora', label: 'LoRA' },
              { value: 'finetune', label: 'Fine-tune', disabled: subject !== 'person' },
              { value: 'pivotal', label: 'Pivotal', disabled: subject !== 'person' },
              { value: 'video', label: 'Video', disabled: subject !== 'person' },
            ]}
            disabled={busy}
          />
        </div>
        <div className="train-identity">
          <Input
            label="Name"
            value={name}
            disabled={busy}
            onChange={(event) => {
              if (!busy) setName(event.target.value);
            }}
            required
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
        </div>
        <Select
          label="Base"
          value={base}
          onValueChange={handleBase}
          options={visibleBases.map((row) => ({ value: row.id, label: row.label }))}
          placeholder="No trainable base installed"
          disabled={busy || visibleBases.length === 0}
          helpText={peopleMissing ? PEOPLE_BUNDLE_HELP : undefined}
        />
        <div className={`train-drops${edit ? ' train-drops--single' : ''}`}>
          <DropZone
            id="train-targets"
            label="Target images"
            prompt={edit ? 'Drop images here, or browse' : 'Drop images, clips, or a folder'}
            hint={
              edit
                ? 'Choose the finished images. You will add one reference and instruction for each target.'
                : 'Choose the images this LoRA should learn from. Clips and folders are fine too.'
            }
            accept={edit ? IMAGE_ACCEPT : MIXED_ACCEPT}
            disabled={busy}
            onFiles={handleDrop}
          />
          {!edit && (
            <DropZone
              id="train-clips"
              label="Video"
              prompt="Drop clips or a folder"
              hint="A clip is sampled above the rate it keeps, so the sharpest frame of each moment wins its slot, repeats of a shot already taken are dropped, and every frame is cropped around whatever moved. Many clips extract at once."
              accept={MIXED_ACCEPT}
              disabled={busy}
              onFiles={handleDrop}
            >
              {(clips.length > 0 || sampling.length > 0) && (
                <ul className="train-clips" aria-label="Accepted clips">
                  {clips.map((clip) => (
                    <li key={clip.key} className="train-clip">
                      <span className="train-clip-receipt">
                        <span className="train-clip-name">{clip.name}</span>: {clip.summary}
                      </span>
                      <Button
                        type="button"
                        size="icon"
                        variant="ghost"
                        disabled={busy}
                        aria-label={`Remove clip ${clip.name}`}
                        onClick={() => removeClip(clip.key)}
                      >
                        <CloseIcon />
                      </Button>
                    </li>
                  ))}
                  {sampling.length === 1 && (
                    <li className="train-clip">Reading {sampling[0].name}…</li>
                  )}
                  {sampling.length > 1 && (
                    <li className="train-clip">Reading {sampling.length} clips…</li>
                  )}
                </ul>
              )}
            </DropZone>
          )}
        </div>
        {!edit && (
          <Checkbox
            label="Mirror half the frames of each second"
            helpText="More variety from one angle, applied as each clip is read. Turn it off for a subject carrying text, or one a mirror would get wrong."
            checked={mirror}
            disabled={busy}
            onCheckedChange={setMirror}
          />
        )}
        {images.length > 0 && !edit && (
          <div className="train-caption">
            <Button
              type="button"
              variant="secondary"
              loading={captioning}
              disabled={busy || captioning}
              onClick={() => void handleCaption()}
            >
              Auto-caption images
            </Button>
            <p className="train-caption-hint">
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
            <List
              rowComponent={TrainPairRow}
              rowCount={images.length}
              rowHeight={PAIR_ROW_HEIGHT}
              rowProps={pairRowProps}
              overscanCount={8}
              className="train-pairs-window"
              style={{ height: listHeight, width: '100%' }}
            />
          </div>
        )}
        <div className="train-footer">
          <Button
            type="submit"
            loading={busy}
            disabled={busy || !ready || sampling.length > 0}
            aria-describedby={edit ? 'train-pairs-status' : undefined}
          >
            Train
          </Button>
        </div>
      </form>
    </section>
  );
}
