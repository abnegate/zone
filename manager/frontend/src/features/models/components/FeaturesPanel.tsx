import { Button, Checkbox, Select } from '@zone/ui';
import { useCallback, useEffect, useRef, useState } from 'react';
import { modelsApi, SetupError, type SetupPlan } from '../../../api/models';
import type { PullApi } from '../hooks/usePull';
import './FeaturesPanel.css';

type FeatureRow = SetupPlan['features'][number];

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    window.setTimeout(resolve, ms);
  });
}

function formatSetupDisk(bytes: number): string {
  const gigabytes = bytes / 1_000_000_000;
  if (gigabytes >= 10) return `${Math.round(gigabytes)} GB`;
  if (gigabytes >= 1) return `${gigabytes.toFixed(1)} GB`;
  const megabytes = bytes / 1_000_000;
  if (megabytes >= 1) return `${Math.round(megabytes)} MB`;
  return `${bytes} B`;
}

function featureSize(feature: FeatureRow): { label: string; state: 'ready' | 'partial' | 'missing' } {
  if (feature.ready || (feature.needed_bytes === 0 && feature.present_bytes > 0)) {
    return { label: 'Installed', state: 'ready' };
  }
  if (feature.present_bytes > 0 && feature.needed_bytes > 0) {
    return { label: `${formatSetupDisk(feature.needed_bytes)} left`, state: 'partial' };
  }
  return { label: feature.size_label, state: 'missing' };
}

function keepInventory(previous: SetupPlan | null, next: SetupPlan): SetupPlan {
  if (!previous) return next;
  const prior = new Map(previous.features.map((feature) => [feature.id, feature]));
  return {
    ...next,
    features: next.features.map((feature) => {
      const known = prior.get(feature.id);
      if (!known) return feature;
      if (feature.present_bytes === 0 && feature.needed_bytes === 0 && known.present_bytes > 0) {
        return {
          ...feature,
          present_bytes: known.present_bytes,
          needed_bytes: known.needed_bytes,
        };
      }
      return feature;
    }),
  };
}

export default function FeaturesPanel({
  pull,
  onInstalled,
  variant = 'page',
}: {
  pull: PullApi;
  onInstalled: () => void;
  variant?: 'page' | 'setup';
}) {
  const [plan, setPlan] = useState<SetupPlan | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [preset, setPreset] = useState('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);
  const notified = useRef(false);

  const load = useCallback(async (features: string[], chatPreset?: string) => {
    setError(null);
    const next = await modelsApi.getSetup({
      features: features.length ? features : undefined,
      chatPreset,
    });
    setPlan((previous) => keepInventory(previous, next));
    setSelected(next.features.filter((feature) => feature.selected).map((feature) => feature.id));
    setPreset(next.chat_preset);
    return next;
  }, []);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    void modelsApi
      .getSetup()
      .then((next) => {
        if (cancelled) return;
        setPlan((previous) => keepInventory(previous, next));
        setSelected(
          next.features.filter((feature) => feature.selected).map((feature) => feature.id)
        );
        setPreset(next.chat_preset);
      })
      .catch((failure: unknown) => {
        if (cancelled) return;
        setError(failure instanceof Error ? failure.message : 'Failed to load feature setup');
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const refreshPlan = useCallback(
    async (features: string[], chatPreset: string) => {
      try {
        await load(features, chatPreset);
      } catch (failure) {
        setError(failure instanceof Error ? failure.message : 'Failed to load feature setup');
      }
    },
    [load]
  );

  const toggle = (id: string, on: boolean) => {
    const feature = plan?.features.find((item) => item.id === id);
    if (!feature || feature.required || (on && feature.blocked)) return;
    const next = on ? [...selected, id] : selected.filter((item) => item !== id);
    const unique = ['chat', ...next.filter((item) => item !== 'chat')];
    setSelected(unique);
    void refreshPlan(unique, preset);
  };

  const changePreset = (value: string) => {
    setPreset(value);
    void refreshPlan(selected, value);
  };

  const install = async () => {
    if (!plan || plan.gate || installing) return;
    setInstalling(true);
    setError(null);
    try {
      const started = await modelsApi.startSetup({ features: selected, chatPreset: preset });
      setPlan((previous) => keepInventory(previous, started));
      setSelected(
        started.features.filter((feature) => feature.selected).map((feature) => feature.id)
      );
      for (const item of started.pulls) {
        while (!pull.canStart(item.model)) {
          await sleep(200);
        }
        void pull.pull(item.model, { runtime: item.runtime === 'comfy' ? 'comfy' : undefined });
      }
      if (started.pulls.length === 0) {
        onInstalled();
      }
    } catch (failure) {
      if (failure instanceof SetupError) {
        setPlan((previous) => keepInventory(previous, failure.plan));
        setSelected(
          failure.plan.features.filter((feature) => feature.selected).map((feature) => feature.id)
        );
        setError(failure.message);
      } else {
        setError(failure instanceof Error ? failure.message : 'Failed to start feature setup');
      }
    } finally {
      setInstalling(false);
    }
  };

  useEffect(() => {
    if (!plan || plan.gate) return;
    if (plan.pulls.length === 0) {
      if (!notified.current) {
        notified.current = true;
        onInstalled();
      }
      return;
    }
    notified.current = false;
    const names = new Set(plan.pulls.map((item) => item.model));
    const ours = pull.jobs.filter((job) => names.has(job.modelName));
    if (ours.length > 0 && ours.every((job) => !job.pulling)) {
      onInstalled();
    }
  }, [onInstalled, plan, pull.jobs]);

  if (loading) {
    return (
      <div className="loading-placeholder">
        <span className="spinner" /> Loading features...
      </div>
    );
  }

  if (!plan) {
    return (
      <div className="error-placeholder" role="alert">
        {error ?? 'Failed to load feature setup'}
      </div>
    );
  }

  const gate = plan.gate;
  const upToDate = !gate && plan.pulls.length === 0;
  const pullingSelected = plan.pulls.some((item) =>
    pull.jobs.some((job) => job.modelName === item.model && job.pulling)
  );

  return (
    <section
      className={`models-features-panel${variant === 'setup' ? ' models-features-panel--setup' : ''}`}
      aria-label="Features"
    >
      {variant === 'setup' ? (
        <p className="models-setup-facts">
          {plan.ram_label} RAM · {plan.disk_free_label} free
        </p>
      ) : (
        <>
          <div className="models-section-head">
            <h2>Features</h2>
            <span className="help-text">
              RAM {plan.ram_label} · {plan.disk_free_label} free · chat {plan.chat_preset}
            </span>
          </div>
          <p className="help-text">
            Pick the product features to download. Chat stays on. Vision needs 16 GB RAM. All is
            blocked when this disk cannot hold the full set.
          </p>
        </>
      )}

      <Select
        label="Chat models"
        value={preset || undefined}
        onValueChange={changePreset}
        options={plan.chat_presets.map((option) => ({
          value: option.id,
          label: option.label,
        }))}
      />

      <div className="models-feature-list">
        {plan.features.map((feature) => {
          const size = featureSize(feature);
          return (
            <div key={feature.id} className="models-feature">
              <Checkbox
                label={feature.label}
                helpText={feature.block_reason ?? feature.description}
                checked={selected.includes(feature.id)}
                disabled={feature.required || (feature.blocked && !selected.includes(feature.id))}
                onCheckedChange={(checked) => toggle(feature.id, checked)}
              />
              <span
                className={`models-feature-size models-feature-size--${size.state}`}
                aria-label={`${feature.label}: ${size.label}`}
              >
                {size.label}
              </span>
            </div>
          );
        })}
      </div>

      <dl className="models-setup-totals">
        <div>
          <dt>All models</dt>
          <dd>{plan.totals.size_label}</dd>
        </div>
        <div>
          <dt>Already on disk</dt>
          <dd>{plan.totals.present_label}</dd>
        </div>
        <div>
          <dt>Still to download</dt>
          <dd>{plan.totals.needed_label}</dd>
        </div>
        <div>
          <dt>Working space</dt>
          <dd>{plan.totals.working_space_label}</dd>
        </div>
        <div>
          <dt>Free required</dt>
          <dd>{plan.totals.required_free_label}</dd>
        </div>
        <div>
          <dt>Free now</dt>
          <dd>{plan.totals.free_now_label}</dd>
        </div>
        {plan.totals.short_by_label && (
          <div>
            <dt>Short by</dt>
            <dd>{plan.totals.short_by_label}</dd>
          </div>
        )}
      </dl>

      {gate && (
        <div className="models-setup-gate" role="alert">
          {gate.message}
        </div>
      )}
      {error && !gate && (
        <div className="models-setup-gate" role="alert">
          {error}
        </div>
      )}

      {plan.licenses.length > 0 && (
        <p className="help-text models-setup-licenses">Licenses: {plan.licenses.join(' · ')}</p>
      )}

      {!(variant === 'setup' && upToDate) && (
        <div className="models-setup-actions">
          <Button
            size={variant === 'setup' ? 'lg' : undefined}
            onClick={() => void install()}
            disabled={Boolean(gate) || upToDate || installing || pullingSelected}
            loading={installing || pullingSelected}
          >
            {upToDate ? 'Up to date' : 'Install selected'}
          </Button>
        </div>
      )}
    </section>
  );
}
