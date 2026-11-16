import { describe, expect, it } from 'bun:test';
import { AnalyticsLabel, NO_MODEL, NO_SOURCE } from './AnalyticsLabel';
import { Tone } from './Tone';

describe('AnalyticsLabel', () => {
  it('names a run with no recorded model for what it is', () => {
    expect(NO_MODEL).toBe('Model not recorded');
    expect(NO_SOURCE).toBe('No source');
  });

  it('names source types the way the source registry does', () => {
    expect(AnalyticsLabel.sourceType('github')).toBe('GitHub');
    expect(AnalyticsLabel.sourceType('gitlab')).toBe('GitLab');
  });

  it('shows a name it has no label for exactly as the server sent it', () => {
    expect(AnalyticsLabel.sourceType('carrier_pigeon')).toBe('carrier_pigeon');
    expect(AnalyticsLabel.taskStatus('webhook_call')).toBe('webhook_call');
    expect(AnalyticsLabel.origin('webhook_call')).toBe('webhook_call');
    expect(AnalyticsLabel.quality('stellar')).toBe('stellar');
  });

  it('labels the vocabularies it knows', () => {
    expect(AnalyticsLabel.category('merge_conflict')).toBe('Merge conflicts');
    expect(AnalyticsLabel.taskStatus('review')).toBe('In review');
    expect(AnalyticsLabel.origin('automation')).toBe('Automations');
    expect(AnalyticsLabel.quality('exemplary')).toBe('Exemplary');
  });

  it('tints what it knows and leaves the rest neutral', () => {
    expect(AnalyticsLabel.taskStatusTone('complete')).toBe(Tone.Success);
    expect(AnalyticsLabel.taskStatusTone('webhook_call')).toBe(Tone.Neutral);
    expect(AnalyticsLabel.qualityTone('poor')).toBe(Tone.Destructive);
    expect(AnalyticsLabel.qualityTone('stellar')).toBe(Tone.Neutral);
    expect(AnalyticsLabel.originTone()).toBe(Tone.Accent);
  });
});
