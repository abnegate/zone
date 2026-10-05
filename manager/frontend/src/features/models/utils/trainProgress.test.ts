import { describe, expect, it } from 'bun:test';
import {
  computeHelp,
  formatEta,
  formatLoss,
  methodAdvice,
  previewSrc,
  trainHeadline,
  trainJobPercent,
  trainPercent,
  trainStepLabel,
} from './trainProgress';

describe('trainProgress', () => {
  it('formats an eta in minutes or hours', () => {
    expect(formatEta(12)).toBe('less than a minute left');
    expect(formatEta(60)).toBe('about 1 minute left');
    expect(formatEta(180)).toBe('about 3 minutes left');
    expect(formatEta(3600)).toBe('about 1 hour left');
    expect(formatEta(7200)).toBe('about 2 hours left');
  });

  it('formats a multi-day fine-tune eta in days', () => {
    expect(formatEta(36 * 3600)).toBe('about 2 days left');
    expect(formatEta(7 * 86400)).toBe('about 7 days left');
  });

  it('turns a step into a percentage', () => {
    expect(trainPercent(12, 400)).toBe(3);
    expect(trainPercent(400, 400)).toBe(100);
    expect(trainPercent(null, 400)).toBeNull();
    expect(trainPercent(0, 400)).toBe(0);
  });

  it('prefers a weighted percent over step/total', () => {
    expect(trainJobPercent({ step: 0, total: 8000, percent: 8 })).toBe(8);
    expect(trainJobPercent({ step: 40, total: 400 })).toBe(10);
    expect(trainJobPercent({ percent: 0, step: 0, total: 8000 })).toBe(0);
  });

  it('names the overlay from the job status and method', () => {
    expect(trainHeadline('running', 'jerry')).toBe('Training jerry');
    expect(trainHeadline('succeeded', 'jerry')).toBe('Training finished jerry');
    expect(trainHeadline('failed', 'jerry')).toBe('Training failed jerry');
    expect(trainHeadline('running', 'jerry', 'finetune')).toBe('Fine-tuning jerry');
    expect(trainHeadline('succeeded', 'jerry', 'finetune')).toBe('Fine-tuning finished jerry');
    expect(trainHeadline('running', 'jerry', 'pivotal')).toBe('Pivotal training jerry');
    expect(trainHeadline('running', 'jerry', 'video')).toBe('Training video jerry');
    expect(trainHeadline('running', 'notes', 'lora', 'language')).toBe(
      'Training language LoRA notes'
    );
    expect(trainHeadline('succeeded', 'notes', 'lora', 'language')).toBe(
      'Training language LoRA finished notes'
    );
    expect(trainHeadline('running', 'notes', 'finetune', 'language')).toBe('Fine-tuning notes');
    expect(trainHeadline('running', 'jerry', 'lora', 'person', 'runpod')).toBe(
      'Training jerry on Runpod'
    );
    expect(trainHeadline('running', 'jerry', 'finetune', 'person', 'runpod', 'A40')).toBe(
      'Fine-tuning jerry on Runpod A40'
    );
    expect(trainHeadline('succeeded', 'jerry', 'lora', 'person', 'runpod', 'A40')).toBe(
      'Training finished jerry on Runpod A40'
    );
    expect(trainHeadline('running', 'jerry', 'lora', 'person', 'local')).toBe('Training jerry');
  });

  it('quotes Runpod compute for every train subject', () => {
    const saveKey = 'Save a Runpod API key in Workspace Settings.';
    const gpu24 = 'A 24 GB GPU is enough for this method.';
    expect(
      computeHelp({ subject: 'other', method: 'lora', provider: 'local', hasKey: false })
    ).toBe(saveKey);
    expect(
      computeHelp({ subject: 'language', method: 'lora', provider: 'local', hasKey: false })
    ).toBe(saveKey);
    expect(
      computeHelp({ subject: 'person', method: 'lora', provider: 'local', hasKey: false })
    ).toBe(saveKey);
    expect(
      computeHelp({ subject: 'person', method: 'lora', provider: 'local', hasKey: true })
    ).toBeNull();
    expect(
      computeHelp({ subject: 'other', method: 'lora', provider: 'local', hasKey: true })
    ).toBeNull();
    expect(
      computeHelp({ subject: 'person', method: 'finetune', provider: 'runpod', hasKey: true })
    ).toBe('Auto-picks a 48 GB GPU (A40 class). Twenty looks per still; hours, a few dollars.');
    expect(
      computeHelp({ subject: 'person', method: 'lora', provider: 'runpod', hasKey: true })
    ).toBe(gpu24);
    expect(
      computeHelp({ subject: 'person', method: 'pivotal', provider: 'runpod', hasKey: true })
    ).toBe(gpu24);
    expect(
      computeHelp({ subject: 'person', method: 'video', provider: 'runpod', hasKey: true })
    ).toBe(gpu24);
    expect(
      computeHelp({ subject: 'language', method: 'lora', provider: 'runpod', hasKey: true })
    ).toBe(gpu24);
    expect(
      computeHelp({ subject: 'other', method: 'lora', provider: 'runpod', hasKey: true })
    ).toBe(gpu24);
  });

  it('uses the phase message as the step label', () => {
    expect(
      trainStepLabel({
        status: 'running',
        step: 0,
        total: 8000,
        message: 'Generating class image 12 of 2000',
      })
    ).toBe('Generating class image 12 of 2000');
    expect(trainStepLabel({ status: 'running', step: 0, total: 400 })).toBe('Starting training');
    expect(trainStepLabel({ status: 'running', step: 40, total: 400 })).toBe('Step 40 of 400');
  });

  it('formats training loss', () => {
    expect(formatLoss(0.2134)).toBe('loss 0.2134');
    expect(formatLoss(null)).toBe('');
  });

  it('advises fine-tune once the drop is large enough', () => {
    expect(methodAdvice({ subject: 'person', method: 'lora', stills: 12, clips: 0 })).toBeNull();
    expect(methodAdvice({ subject: 'person', method: 'lora', stills: 220, clips: 0 })).toMatch(
      /switch to fine-tune/
    );
    expect(methodAdvice({ subject: 'person', method: 'lora', stills: 2000, clips: 1000 })).toMatch(
      /full fine-tune will beat LoRA/
    );
    expect(methodAdvice({ subject: 'person', method: 'finetune', stills: 12, clips: 0 })).toMatch(
      /train a LoRA first/
    );
    expect(
      methodAdvice({ subject: 'person', method: 'finetune', stills: 2000, clips: 1000 })
    ).toMatch(/strong fine-tune range/);
    expect(methodAdvice({ subject: 'other', method: 'lora', stills: 2000, clips: 0 })).toBeNull();
    expect(
      methodAdvice({ subject: 'language', method: 'lora', stills: 2000, clips: 0 })
    ).toBeNull();
    expect(methodAdvice({ subject: 'person', method: 'pivotal', stills: 80, clips: 0 })).toBeNull();
    expect(methodAdvice({ subject: 'person', method: 'video', stills: 0, clips: 40 })).toMatch(
      /trains Wan from the clips/
    );
  });

  it('turns a relative preview path into a train preview url', () => {
    expect(previewSrc('previews/step-250-0.png')).toBe('/api/models/train/previews/step-250-0.png');
    expect(previewSrc('previews\\step-250-0.png')).toBe(
      '/api/models/train/previews/step-250-0.png'
    );
    expect(previewSrc('/api/models/train/previews/already.png')).toBe(
      '/api/models/train/previews/already.png'
    );
    expect(previewSrc('https://example.test/p.png')).toBe('https://example.test/p.png');
    expect(previewSrc('data:image/png;base64,abc')).toBe('data:image/png;base64,abc');
  });
});
