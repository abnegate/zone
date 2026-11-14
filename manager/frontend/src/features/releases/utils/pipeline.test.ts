import { describe, expect, it } from 'bun:test';
import { listing, pipelineRun, release, SOURCE_ID, verdict } from '../../../test/releases';
import { PIPELINE_STATES } from '../schemas';
import type { Release } from '../types';
import {
  newerPipelines,
  PIPELINE_LABELS,
  PIPELINE_TINTS,
  PIPELINE_WATCH_MS,
  pipelineActive,
  pipelineRecent,
  pipelineWatched,
  verdictOf,
  withPipeline,
} from './pipeline';

const published = '2026-11-13T09:00:00Z';
const publishedAt = Date.parse(published);
const MINUTE = 60_000;

describe('pipeline tints and labels', () => {
  it('has a tint and a label for every state', () => {
    for (const state of PIPELINE_STATES) {
      expect(PIPELINE_TINTS[state]).toBeString();
      expect(PIPELINE_LABELS[state]).toBeString();
    }
  });

  it('tints outcomes by how they ended', () => {
    expect(PIPELINE_TINTS.succeeded).toBe('success');
    expect(PIPELINE_TINTS.failed).toBe('destructive');
    expect(PIPELINE_TINTS['timed-out']).toBe('destructive');
    expect(PIPELINE_TINTS.running).toBe('info');
    expect(PIPELINE_TINTS.queued).toBe('info');
    expect(PIPELINE_TINTS['action-required']).toBe('warning');
  });

  it('labels kebab-case states in words', () => {
    expect(PIPELINE_LABELS['action-required']).toBe('Action required');
    expect(PIPELINE_LABELS['timed-out']).toBe('Timed out');
  });
});

describe('pipelineActive', () => {
  it('is active while the lookup is pending', () => {
    expect(pipelineActive(release({ lookup: 'pending', runs: [] }).pipeline)).toBe(true);
  });

  it.each(['queued', 'running'] as const)('is active while a run is %s', (state) => {
    const runs = [pipelineRun(), pipelineRun({ id: 8, state })];
    expect(pipelineActive(release({ runs }).pipeline)).toBe(true);
  });

  it('is settled once the lookup completed and every run finished', () => {
    const runs = [pipelineRun(), pipelineRun({ id: 8, state: 'failed' })];
    expect(pipelineActive(release({ runs }).pipeline)).toBe(false);
  });

  it('is settled when the lookup is unavailable and nothing runs', () => {
    expect(pipelineActive(release({ lookup: 'unavailable', runs: [] }).pipeline)).toBe(false);
  });
});

describe('pipelineWatched', () => {
  const active = release({ published_at: published, lookup: 'pending', runs: [] });

  it('watches an active release younger than 30 minutes', () => {
    expect(pipelineWatched(active, publishedAt + 29 * MINUTE)).toBe(true);
  });

  it('stops watching at 30 minutes', () => {
    expect(pipelineWatched(active, publishedAt + PIPELINE_WATCH_MS)).toBe(false);
  });

  it('never watches a settled release, however young', () => {
    expect(pipelineWatched(release({ published_at: published }), publishedAt + MINUTE)).toBe(false);
  });

  it('watches a release published slightly ahead of the local clock', () => {
    expect(pipelineWatched(active, publishedAt - MINUTE)).toBe(true);
  });

  it('never watches a release whose date cannot be read', () => {
    expect(pipelineRecent('not a date', publishedAt)).toBe(false);
  });
});

describe('verdictOf', () => {
  it('reads the pipeline response a release already carries', () => {
    const subject = release({ state: 'running', succeeded: false });
    expect(verdictOf(subject)).toEqual(verdict(subject));
  });
});

describe('withPipeline', () => {
  const pending = release({ id: 42, lookup: 'pending', runs: [], state: 'unknown' });
  const other = release({ id: 41, tag: 'v1.2.2' });

  it('patches the matching release with the new pipeline and verdict', () => {
    const settled = release({ id: 42, state: 'failed', succeeded: false });
    const update = verdict(settled, {
      blockers: [{ code: 'runs_failed', detail: '1 run failed' }],
    });
    const patched = withPipeline(listing([pending, other]), SOURCE_ID, update);
    expect(patched.releases[0]).toMatchObject({
      id: 42,
      state: 'failed',
      succeeded: false,
      blockers: update.blockers,
      pipeline: settled.pipeline,
    });
    expect(patched.releases[1]).toBe(other);
  });

  it('leaves a listing without the release untouched', () => {
    const before = listing([other]);
    expect(withPipeline(before, SOURCE_ID, verdict(pending))).toBe(before);
  });

  it('matches on the source as well as the release id', () => {
    const before = listing([pending]);
    expect(withPipeline(before, 'another-source', verdict(pending))).toBe(before);
  });

  it('does not change the listing it was given', () => {
    const before = listing([pending]);
    withPipeline(before, SOURCE_ID, verdict(release({ id: 42 })));
    expect(before.releases[0]).toBe(pending);
  });
});

describe('newerPipelines', () => {
  function at(subject: Release, checked: string, state: Release['state']): Release {
    return {
      ...subject,
      state,
      pipeline: { ...subject.pipeline, runs: [pipelineRun({ state })], checked_at: checked },
    };
  }

  const early = '2026-11-13T09:05:00Z';
  const late = '2026-11-13T09:10:00Z';

  it('keeps the verdict of a pipeline the cache checked later', () => {
    const cached = listing([at(release(), late, 'succeeded')]);
    const polled = listing([at(release(), early, 'running')]);
    const merged = newerPipelines(polled, cached);
    expect(merged.releases[0].state).toBe('succeeded');
    expect(merged.releases[0].pipeline.checked_at).toBe(late);
  });

  it('takes the polled pipeline when it is as new or newer', () => {
    const polled = listing([at(release(), late, 'succeeded')]);
    expect(newerPipelines(polled, listing([at(release(), early, 'running')]))).toBe(polled);
    expect(newerPipelines(polled, listing([at(release(), late, 'running')]))).toBe(polled);
  });

  it('takes the polled listing when nothing was cached or the release is new', () => {
    const polled = listing([at(release(), early, 'running')]);
    expect(newerPipelines(polled, undefined)).toBe(polled);
    expect(newerPipelines(polled, listing([at(release({ id: 7 }), late, 'succeeded')]))).toBe(
      polled
    );
  });
});
