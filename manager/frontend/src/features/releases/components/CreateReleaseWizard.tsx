import { Button, Select, Wizard, type WizardStep } from '@zone/ui';
import { type ReactElement, useEffect, useRef, useState } from 'react';
import { Link } from 'react-router-dom';
import { MessageContent } from '../../chats';
import { outcomeUnknown, shortSha } from '../../github';
import { SOURCES_PATH } from '../../settings';
import { useCreateRelease } from '../hooks/useCreateRelease';
import { useReleaseOptions } from '../hooks/useReleaseOptions';
import { ReleaseRefusal } from '../ReleaseRefusal';
import type {
  Created,
  Preview,
  PreviewRequest,
  Reconciliation,
  SourceOptions,
  TagLine,
} from '../types';
import { dominantLine, expectedLatestTag, lineLabel, lineOfTag, tagLines } from '../utils/lines';
import { RECONCILIATION_NEEDED } from '../utils/reconciliation';
import { parseTag, tagProblem } from '../utils/tags';
import { ReleaseWizardFailure } from './ReleaseWizardFailure';

const TARGET = 0;

const REVIEW = 1;

const STEPS: WizardStep[] = [
  { id: 'target', title: 'Target', description: 'Repository and tag' },
  { id: 'review', title: 'Review', description: 'Notes and pull requests' },
];

const PREVIOUS_TAG_LIMIT = 5;

const TAG_INPUT = 'release-tag';

const TAG_HINT = 'release-tag-hint';

const TAG_KIND = 'release-tag-kind';

const TAG_ERROR = 'release-tag-error';

export const RELOAD_OPTIONS = 'Reload options';
export const REVIEW_LABEL = 'Review';
export const PREVIEWING = 'Previewing...';
export const PUBLISH = 'Publish';
export const PUBLISHING = 'Publishing...';
export const PREVIEW_FAILED = 'Failed to preview the release';
export const CREATE_FAILED = 'Failed to publish the release';
export const CREATE_OUTCOME_UNKNOWN =
  'The release stopped before it reported back, so it may have been published. Check the releases on GitHub before trying again.';
export const RELOAD_FAILED = 'Failed to reload release options';
export const LOADING_OPTIONS = 'Loading release options...';
export const NO_SOURCES = 'No GitHub sources to release from';
export const CONNECT_SOURCE = 'Connect a GitHub source';
export const FIRST_RELEASE = 'First release';
export const NO_PREVIOUS_TAGS = 'No earlier tags';
export const NO_INCLUDED_PULLS = 'No pull requests since the last release';
export const LINE_LABEL = 'Line';
export const PRERELEASE_TAG =
  'Publishes as a pre-release, because the tag has a pre-release suffix';
export const STABLE_TAG = 'Publishes as a stable release';

export function previousTags(tags: readonly string[]): string {
  if (tags.length === 0) return NO_PREVIOUS_TAGS;
  return `Previous tags: ${tags.slice(0, PREVIOUS_TAG_LIMIT).join(', ')}`;
}

interface Failure {
  message: string;
  refused: boolean;
  reconciliation: Reconciliation | null;
  unknown: boolean;
}

interface LineChoice {
  sourceId: string;
  prefix: string | null;
}

interface Draft extends LineChoice {
  tag: string;
}

interface Previewed {
  request: PreviewRequest;
  preview: Preview;
}

function failureOf(error: unknown, fallback: string, writing = false): Failure {
  const refusal = error instanceof ReleaseRefusal ? error : null;
  const reconciliation = refusal?.reconciliation ?? null;
  const unknown = writing && outcomeUnknown(error);
  const reported = error instanceof Error ? error.message : fallback;
  const message = reconciliation
    ? RECONCILIATION_NEEDED
    : unknown
      ? CREATE_OUTCOME_UNKNOWN
      : reported;
  return { message, refused: refusal !== null, reconciliation, unknown };
}

function chooseSource(
  sources: readonly SourceOptions[],
  preferred: string | null | undefined
): SourceOptions | undefined {
  return sources.find((source) => source.source_id === preferred) ?? sources[0];
}

function chooseLine(source: SourceOptions, choice: LineChoice | null): TagLine {
  const chosen =
    choice?.sourceId === source.source_id
      ? tagLines(source).find((line) => line.prefix === choice.prefix)
      : undefined;
  return chosen ?? dominantLine(source);
}

function sameLine(choice: LineChoice | null, sourceId: string, line: TagLine): boolean {
  return choice?.sourceId === sourceId && choice.prefix === line.prefix;
}

interface CreateReleaseWizardProps {
  isOpen: boolean;
  onClose: () => void;
  onCreated: (created: Created) => void;
  sourceId?: string;
}

export function CreateReleaseWizard({
  isOpen,
  onClose,
  onCreated,
  sourceId,
}: CreateReleaseWizardProps): ReactElement {
  const [step, setStep] = useState(TARGET);
  const [chosen, setChosen] = useState<string | null>(null);
  const [lineChoice, setLineChoice] = useState<LineChoice | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [previewed, setPreviewed] = useState<Previewed | null>(null);
  const [failure, setFailure] = useState<Failure | null>(null);
  const publishing = useRef(false);
  const tagInput = useRef<HTMLInputElement>(null);

  const {
    sources,
    loading: optionsLoading,
    error: optionsError,
    reload,
    reloading,
  } = useReleaseOptions(isOpen);
  const { preview, previewing, create, creating, reset } = useCreateRelease();

  const source = chooseSource(sources, chosen ?? sourceId);
  const lines = source ? tagLines(source) : [];
  const line = source ? chooseLine(source, lineChoice) : null;
  const tag =
    source && line && sameLine(draft, source.source_id, line)
      ? (draft?.tag ?? line.next_tag)
      : (line?.next_tag ?? '');
  const parsed = parseTag(tag);
  const problem = source ? tagProblem(tag, lineOfTag(source, tag)?.previous_tags ?? []) : null;
  const busy = previewing || creating || reloading;
  const reconciling = failure?.reconciliation != null;
  const canProceed =
    step === TARGET
      ? !!source && problem === null && !optionsLoading
      : previewed !== null && !reconciling;
  const tagShown = isOpen && step === TARGET && !optionsLoading && !!source;

  useEffect(() => {
    if (tagShown) tagInput.current?.focus();
  }, [tagShown]);

  const clear = () => {
    setStep(TARGET);
    setChosen(null);
    setLineChoice(null);
    setDraft(null);
    setPreviewed(null);
    setFailure(null);
    reset();
  };

  const close = () => {
    if (busy) return;
    clear();
    onClose();
  };

  const review = async () => {
    if (!source) return;
    const proposed = tag.trim();
    const request: PreviewRequest = {
      source_id: source.source_id,
      tag: proposed,
      expected_latest_tag: expectedLatestTag(source, proposed),
    };
    setFailure(null);
    try {
      setPreviewed({ request, preview: await preview(request) });
      setStep(REVIEW);
    } catch (error) {
      setFailure(failureOf(error, PREVIEW_FAILED));
    }
  };

  const changeStep = (next: number) => {
    if (next === REVIEW) {
      void review();
      return;
    }
    setStep(TARGET);
    setPreviewed(null);
    setFailure(null);
  };

  const publish = async () => {
    if (!previewed || reconciling || publishing.current) return;
    publishing.current = true;
    const { request, preview: notes } = previewed;
    setFailure(null);
    try {
      const created = await create({
        ...request,
        prerelease: notes.prerelease,
        digest: notes.digest,
      });
      clear();
      onCreated(created);
      onClose();
    } catch (error) {
      if (error instanceof ReleaseRefusal && error.tagRefusal) {
        setStep(TARGET);
        setPreviewed(null);
      }
      setFailure(failureOf(error, CREATE_FAILED, true));
    } finally {
      publishing.current = false;
    }
  };

  const reloadOptions = async () => {
    setStep(TARGET);
    setDraft(null);
    setPreviewed(null);
    setFailure(null);
    reset();
    try {
      await reload();
    } catch (error) {
      setFailure(failureOf(error, RELOAD_FAILED));
    }
  };

  const selectSource = (value: string) => {
    setChosen(value);
    setFailure(null);
  };

  const selectLine = (value: string) => {
    const next = lines[Number(value)];
    if (source && next) setLineChoice({ sourceId: source.source_id, prefix: next.prefix });
    setFailure(null);
  };

  const editTag = (value: string) => {
    if (source && line) setDraft({ sourceId: source.source_id, prefix: line.prefix, tag: value });
    setFailure(null);
  };

  const reloadButton = (
    <Button variant="secondary" size="sm" onClick={reloadOptions} disabled={busy}>
      {RELOAD_OPTIONS}
    </Button>
  );

  const renderTarget = () => {
    if (optionsLoading) {
      return (
        <p className="release-wizard-status" role="status">
          {LOADING_OPTIONS}
        </p>
      );
    }
    if (!source || !line) {
      if (optionsError) {
        return (
          <ReleaseWizardFailure
            message={optionsError}
            reconciliation={null}
            repositoryUrl={null}
            action={reloadButton}
          />
        );
      }
      return (
        <div className="wizard-empty-state">
          <p>{NO_SOURCES}</p>
          <Link to={SOURCES_PATH} onClick={close}>
            {CONNECT_SOURCE}
          </Link>
        </div>
      );
    }
    const kind = parsed ? TAG_KIND : null;
    const describedBy = [TAG_HINT, kind, problem ? TAG_ERROR : null].filter(Boolean).join(' ');
    return (
      <>
        <p className="wizard-step-intro">
          Choose the repository and the tag to publish. The release notes are generated from the
          pull requests merged since the last tag.
        </p>
        <div className="form-group">
          <Select
            id="release-source"
            label="Repository"
            value={source.source_id}
            onValueChange={selectSource}
            disabled={busy}
            options={sources.map((option) => ({
              value: option.source_id,
              label: option.repository,
            }))}
          />
        </div>
        {lines.length > 1 && (
          <div className="form-group">
            <Select
              id="release-line"
              label={LINE_LABEL}
              value={String(lines.indexOf(line))}
              onValueChange={selectLine}
              disabled={busy}
              options={lines.map((option, index) => ({
                value: String(index),
                label: lineLabel(option),
              }))}
            />
          </div>
        )}
        <div className="form-group">
          <label htmlFor={TAG_INPUT}>Tag</label>
          <input
            id={TAG_INPUT}
            type="text"
            className={problem ? 'release-tag-input input-error' : 'release-tag-input'}
            value={tag}
            onChange={(event) => editTag(event.target.value)}
            aria-invalid={problem !== null}
            aria-describedby={describedBy}
            ref={tagInput}
            autoComplete="off"
            spellCheck={false}
            disabled={busy}
          />
          <span id={TAG_HINT} className="form-hint release-previous-tags">
            {previousTags(line.previous_tags)}
          </span>
          {parsed && (
            <span id={TAG_KIND} className="form-hint release-tag-kind">
              {parsed.prerelease ? PRERELEASE_TAG : STABLE_TAG}
            </span>
          )}
          {problem && (
            <span id={TAG_ERROR} className="field-error">
              {problem}
            </span>
          )}
        </div>
      </>
    );
  };

  const renderReview = () => {
    if (!previewed) return null;
    const { preview: notes } = previewed;
    return (
      <>
        <dl className="detail-facts release-wizard-facts">
          <dt className="detail-label">Repository</dt>
          <dd className="detail-value">{notes.repository}</dd>
          <dt className="detail-label">Tag</dt>
          <dd className="detail-value release-wizard-mono">{notes.tag}</dd>
          <dt className="detail-label">Since</dt>
          <dd className="detail-value release-wizard-mono">{notes.base_tag ?? FIRST_RELEASE}</dd>
          <dt className="detail-label">Target</dt>
          <dd className="detail-value release-wizard-mono" title={notes.target}>
            {shortSha(notes.target)}
          </dd>
          <dt className="detail-label">Pre-release</dt>
          <dd className="detail-value">{notes.prerelease ? 'Yes' : 'No'}</dd>
        </dl>
        <section className="release-wizard-section" aria-labelledby="release-wizard-pulls">
          <h3 id="release-wizard-pulls">Pull requests</h3>
          {notes.pulls.length > 0 ? (
            <ul className="release-wizard-pulls">
              {notes.pulls.map((pull) => (
                <li key={pull.number} className="release-card-pull">
                  <a href={pull.url} target="_blank" rel="noreferrer">
                    <span className="release-card-pull-number">#{pull.number}</span>
                    <span className="release-card-pull-title">{pull.title}</span>
                  </a>
                </li>
              ))}
            </ul>
          ) : (
            <p className="release-wizard-empty">{NO_INCLUDED_PULLS}</p>
          )}
        </section>
        <section className="release-wizard-section" aria-labelledby="release-wizard-notes">
          <h3 id="release-wizard-notes">Notes</h3>
          <div className="release-wizard-notes">
            <MessageContent content={notes.body} links="all" compact />
          </div>
        </section>
      </>
    );
  };

  const written = failure && (failure.reconciliation !== null || failure.unknown);

  return (
    <Wizard
      isOpen={isOpen}
      onClose={close}
      title="New release"
      subtitle="Tag a commit and publish its release notes"
      className="release-wizard"
      steps={STEPS}
      currentStep={step}
      onStepChange={changeStep}
      onComplete={publish}
      nextLabel={previewing ? PREVIEWING : REVIEW_LABEL}
      completeLabel={creating ? PUBLISHING : PUBLISH}
      loading={busy}
      canProceed={canProceed}
      allowStepClick
    >
      {step === TARGET ? renderTarget() : renderReview()}
      {failure && (
        <ReleaseWizardFailure
          message={failure.message}
          reconciliation={failure.reconciliation}
          repositoryUrl={written ? (source?.repository_url ?? null) : null}
          action={failure.refused ? reloadButton : undefined}
        />
      )}
    </Wizard>
  );
}
