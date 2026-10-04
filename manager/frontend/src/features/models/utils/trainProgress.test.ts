import { describe, expect, it } from 'bun:test';
import {
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
