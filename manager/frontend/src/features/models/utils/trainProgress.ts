export const LORA_MIN_STILLS = 20;
export const FINE_TUNE_MIN_STILLS = 200;
export const FINE_TUNE_MIN_CLIPS = 20;
export const FINE_TUNE_READY_STILLS = 500;
export const FINE_TUNE_READY_CLIPS = 50;

export type TrainMethodKind = 'lora' | 'finetune' | 'pivotal' | 'video';
export type TrainSubjectKind = 'person' | 'other' | 'language';
export type TrainProviderKind = 'local' | 'runpod';

export type TrainProgressJob = {
  status?: string;
  name?: string | null;
  method?: string | null;
  step?: number | null;
  total?: number | null;
  percent?: number | null;
  phase?: string | null;
  message?: string | null;
  loss?: number | null;
  eta_seconds?: number | null;
  previews?: string[] | null;
};

export function formatEta(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '';
  if (seconds < 45) return 'less than a minute left';
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) {
    return minutes === 1 ? 'about 1 minute left' : `about ${minutes} minutes left`;
  }
  const hours = Math.round(seconds / 3600);
  if (hours < 36) {
    return hours === 1 ? 'about 1 hour left' : `about ${hours} hours left`;
  }
  const days = Math.max(1, Math.round(seconds / 86400));
  return days === 1 ? 'about 1 day left' : `about ${days} days left`;
}

export function trainPercent(step?: number | null, total?: number | null): number | null {
  if (total == null || total <= 0 || step == null || step < 0) return null;
  return Math.min(100, Math.max(0, Math.round((step / total) * 100)));
}

export function trainJobPercent(job: TrainProgressJob): number | null {
  if (job.percent != null && Number.isFinite(job.percent)) {
    return Math.min(100, Math.max(0, Math.round(job.percent)));
  }
  return trainPercent(job.step, job.total);
}

export function trainHeadline(
  status?: string,
  name?: string | null,
  method?: string | null,
  subject?: string | null,
  provider?: string | null,
  gpu?: string | null
): string {
  const verb =
    method === 'finetune'
      ? 'Fine-tuning'
      : method === 'pivotal'
        ? 'Pivotal training'
        : method === 'video'
          ? 'Training video'
          : subject === 'language'
            ? 'Training language LoRA'
            : 'Training';
  const suffix = name?.trim() ? ` ${name.trim()}` : '';
  const gpuName = gpu?.trim();
  const compute = provider === 'runpod' ? (gpuName ? ` on Runpod ${gpuName}` : ' on Runpod') : '';
  if (status === 'succeeded') return `${verb} finished${suffix}${compute}`;
  if (status === 'failed') return `${verb} failed${suffix}${compute}`;
  return `${verb}${suffix}${compute}`;
}

export function computeHelp(input: {
  subject: TrainSubjectKind;
  method: TrainMethodKind;
  provider: TrainProviderKind;
  hasKey: boolean;
}): string | null {
  if (!input.hasKey) return 'Save a Runpod API key in Workspace Settings.';
  if (input.provider !== 'runpod') return null;
  if (input.method === 'finetune') {
    return 'Auto-picks a 48 GB GPU (A40 class). About 4 hours, $1–2.';
  }
  return 'A 24 GB GPU is enough for this method.';
}

export function trainStepLabel(job: TrainProgressJob): string | null {
  if (job.message?.trim()) return job.message.trim();
  if (job.total != null && job.step != null) {
    if (job.step === 0) {
      if (job.phase === 'queued') return 'Waiting for the host trainer';
      return 'Starting training';
    }
    return `Step ${job.step} of ${job.total}`;
  }
  if (job.status === 'running') return 'Preparing the dataset';
  return null;
}

export function formatLoss(loss?: number | null): string {
  if (loss == null || !Number.isFinite(loss)) return '';
  const rounded = loss >= 1 ? loss.toFixed(3) : loss.toFixed(4);
  return `loss ${rounded}`;
}

export function previewSrc(value: string): string {
  if (
    value.startsWith('data:') ||
    value.startsWith('http://') ||
    value.startsWith('https://') ||
    value.startsWith('/')
  ) {
    return value;
  }
  const name = value.split(/[\\/]/).pop() ?? value;
  return `/api/models/train/previews/${encodeURIComponent(name)}`;
}

export function methodAdvice(input: {
  subject: TrainSubjectKind;
  method: TrainMethodKind;
  stills: number;
  clips: number;
}): string | null {
  if (input.subject !== 'person') return null;
  const stills = Math.max(0, input.stills);
  const clips = Math.max(0, input.clips);
  const ready = stills >= FINE_TUNE_READY_STILLS || clips >= FINE_TUNE_READY_CLIPS;
  const consider = stills >= FINE_TUNE_MIN_STILLS || clips >= FINE_TUNE_MIN_CLIPS;
  if (input.method === 'video') {
    return 'This method trains Wan from the clips (2–3 windows per clip). Run a still method too for image identity.';
  }
  if (input.method === 'pivotal') {
    return null;
  }
  if (input.method === 'lora') {
    if (ready) {
      return `This set (${stills} stills, ${clips} clips) is large enough that a full fine-tune will beat LoRA on likeness, hands, and body.`;
    }
    if (consider) {
      return `Around ${FINE_TUNE_MIN_STILLS} unique stills or ${FINE_TUNE_MIN_CLIPS} clips, switch to fine-tune. This set is ${stills} stills and ${clips} clips — LoRA still trains, but a UNet run is what locks hands and body.`;
    }
    return null;
  }
  if (ready) {
    return `This dump (${stills} stills, ${clips} clips) is in the strong fine-tune range.`;
  }
  if (consider) {
    return null;
  }
  return `Fine-tune wants about ${FINE_TUNE_MIN_STILLS} unique stills or ${FINE_TUNE_MIN_CLIPS} clips. Below that, train a LoRA first.`;
}
