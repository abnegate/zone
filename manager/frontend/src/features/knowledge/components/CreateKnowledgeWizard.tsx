import type { WizardStep } from '@zone/ui';
import { useCallback, useMemo, useState } from 'react';
import { Wizard } from '../../../components';
import { useWorkspace } from '../../../shared/context';
import { getErrors } from '../../../validation';
import { CreateKnowledgeRequestSchema } from '../schemas';
import type { CreateKnowledgeRequest, KnowledgeEntry, KnowledgeType } from '../types';

interface CreateKnowledgeWizardProps {
  isOpen: boolean;
  onClose: () => void;
  onCreated: (entry: KnowledgeEntry) => void;
  createEntry: (request: CreateKnowledgeRequest) => Promise<KnowledgeEntry>;
}

const WIZARD_STEPS: WizardStep[] = [
  {
    id: 'type',
    title: 'Type',
    description: 'Choose content type',
  },
  {
    id: 'content',
    title: 'Content',
    description: 'Add your content',
  },
  {
    id: 'details',
    title: 'Details',
    description: 'Title and tags',
  },
];

export function CreateKnowledgeWizard({
  isOpen,
  onClose,
  onCreated,
  createEntry,
}: CreateKnowledgeWizardProps) {
  const { currentWorkspace } = useWorkspace();
  const [currentStep, setCurrentStep] = useState(0);
  const [type, setType] = useState<KnowledgeType>('text');
  const [title, setTitle] = useState('');
  const [content, setContent] = useState('');
  const [tags, setTags] = useState<string[]>([]);
  const [tagInput, setTagInput] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

  const handleStepChange = useCallback((step: number) => {
    setCurrentStep(step);
    setError(null);
  }, []);

  const canProceed = useMemo(() => {
    if (currentStep === 0) {
      return !!type;
    }
    if (currentStep === 1) {
      return content.trim().length > 0;
    }
    if (currentStep === 2) {
      return title.trim().length > 0;
    }
    return true;
  }, [currentStep, type, content, title]);

  const handleComplete = useCallback(async () => {
    if (!currentWorkspace) {
      setError('No workspace selected. Please select or create a workspace first.');
      return;
    }

    const request: CreateKnowledgeRequest = {
      workspace_id: currentWorkspace.id,
      title: title.trim(),
      type,
      content: content.trim(),
      tags: tags.length > 0 ? tags : undefined,
    };

    const errors = getErrors(CreateKnowledgeRequestSchema, request);
    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      return;
    }

    setFieldErrors({});
    setLoading(true);
    setError(null);

    try {
      const entry = await createEntry(request);
      onCreated(entry);
      handleClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create knowledge entry');
    } finally {
      setLoading(false);
    }
  }, [currentWorkspace, title, type, content, tags, createEntry, onCreated]);

  const handleClose = useCallback(() => {
    setCurrentStep(0);
    setType('text');
    setTitle('');
    setContent('');
    setTags([]);
    setTagInput('');
    setError(null);
    setFieldErrors({});
    onClose();
  }, [onClose]);

  const handleAddTag = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === 'Enter' && tagInput.trim()) {
        e.preventDefault();
        if (!tags.includes(tagInput.trim())) {
          setTags([...tags, tagInput.trim()]);
        }
        setTagInput('');
      }
    },
    [tagInput, tags]
  );

  const handleRemoveTag = useCallback(
    (tag: string) => {
      setTags(tags.filter((t) => t !== tag));
    },
    [tags]
  );

  const renderStepContent = () => {
    switch (currentStep) {
      case 0:
        return (
          <div className="wizard-step-content">
            <p className="wizard-step-intro">
              Choose what type of knowledge you want to add. Text content is stored directly, while
              URLs are fetched and their content is extracted.
            </p>
            <div className="knowledge-type-grid">
              <button
                type="button"
                className={`knowledge-type-option ${type === 'text' ? 'selected' : ''}`}
                onClick={() => setType('text')}
              >
                <div className="knowledge-type-icon text">
                  <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    width="20"
                    height="20"
                  >
                    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                    <polyline points="14 2 14 8 20 8" />
                    <line x1="16" y1="13" x2="8" y2="13" />
                    <line x1="16" y1="17" x2="8" y2="17" />
                    <polyline points="10 9 9 9 8 9" />
                  </svg>
                </div>
                <div className="knowledge-type-info">
                  <span className="knowledge-type-name">Text Content</span>
                  <span className="knowledge-type-desc">
                    Store text directly in the knowledge base
                  </span>
                </div>
              </button>
              <button
                type="button"
                className={`knowledge-type-option ${type === 'url' ? 'selected' : ''}`}
                onClick={() => setType('url')}
              >
                <div className="knowledge-type-icon url">
                  <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    width="20"
                    height="20"
                  >
                    <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" />
                    <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" />
                  </svg>
                </div>
                <div className="knowledge-type-info">
                  <span className="knowledge-type-name">URL / Web Page</span>
                  <span className="knowledge-type-desc">Fetch and extract content from a URL</span>
                </div>
              </button>
            </div>
          </div>
        );

      case 1:
        return (
          <div className="wizard-step-content">
            <p className="wizard-step-intro">
              {type === 'url'
                ? 'Enter the URL of the web page you want to add. The content will be fetched and stored.'
                : 'Enter the text content you want to add to the knowledge base.'}
            </p>
            <div className="form-group">
              <label htmlFor="knowledge-content">{type === 'url' ? 'URL' : 'Content'}</label>
              {type === 'url' ? (
                <input
                  type="url"
                  id="knowledge-content"
                  value={content}
                  onChange={(e) => {
                    setContent(e.target.value);
                    if (fieldErrors.content) {
                      setFieldErrors((prev) => {
                        const { content: _content, ...next } = prev;
                        return next;
                      });
                    }
                  }}
                  placeholder="https://example.com/article"
                  className={fieldErrors.content ? 'input-error' : ''}
                />
              ) : (
                <textarea
                  id="knowledge-content"
                  value={content}
                  onChange={(e) => {
                    setContent(e.target.value);
                    if (fieldErrors.content) {
                      setFieldErrors((prev) => {
                        const { content: _content, ...next } = prev;
                        return next;
                      });
                    }
                  }}
                  placeholder="Enter your text content here..."
                  rows={8}
                  className={fieldErrors.content ? 'input-error' : ''}
                />
              )}
              {fieldErrors.content && <span className="field-error">{fieldErrors.content}</span>}
            </div>
          </div>
        );

      case 2:
        return (
          <div className="wizard-step-content">
            <p className="wizard-step-intro">
              Give your knowledge entry a title and optionally add tags for organization.
            </p>
            <div className="form-group">
              <label htmlFor="knowledge-title">Title</label>
              <input
                type="text"
                id="knowledge-title"
                value={title}
                onChange={(e) => {
                  setTitle(e.target.value);
                  if (fieldErrors.title) {
                    setFieldErrors((prev) => {
                      const { title: _title, ...next } = prev;
                      return next;
                    });
                  }
                }}
                placeholder="Give this entry a descriptive title"
                className={fieldErrors.title ? 'input-error' : ''}
              />
              {fieldErrors.title && <span className="field-error">{fieldErrors.title}</span>}
            </div>
            <div className="form-group">
              <label htmlFor="knowledge-tags">
                Tags
                <span className="label-optional">optional</span>
              </label>
              <div className="tag-input-wrapper">
                {tags.map((tag) => (
                  <span key={tag} className="tag-item">
                    {tag}
                    <button
                      type="button"
                      className="tag-remove"
                      onClick={() => handleRemoveTag(tag)}
                      aria-label={`Remove tag ${tag}`}
                    >
                      <svg
                        fill="none"
                        stroke="currentColor"
                        viewBox="0 0 24 24"
                        aria-hidden="true"
                        width="16"
                        height="16"
                      >
                        <path
                          strokeLinecap="round"
                          strokeLinejoin="round"
                          strokeWidth={2}
                          d="M6 18L18 6M6 6l12 12"
                        />
                      </svg>
                    </button>
                  </span>
                ))}
                <input
                  id="knowledge-tags"
                  type="text"
                  value={tagInput}
                  onChange={(e) => setTagInput(e.target.value)}
                  onKeyDown={handleAddTag}
                  placeholder={tags.length === 0 ? 'Press Enter to add tags...' : 'Add more...'}
                />
              </div>
              <span className="form-hint">Press Enter to add each tag</span>
            </div>
          </div>
        );

      default:
        return null;
    }
  };

  return (
    <Wizard
      isOpen={isOpen}
      onClose={handleClose}
      title="Add Knowledge Entry"
      subtitle="Add content to your AI knowledge base"
      steps={WIZARD_STEPS}
      currentStep={currentStep}
      onStepChange={handleStepChange}
      onComplete={handleComplete}
      onCancel={handleClose}
      completeLabel={loading ? 'Creating...' : 'Create Entry'}
      loading={loading}
      canProceed={canProceed}
      allowStepClick
    >
      {renderStepContent()}
      {Object.keys(fieldErrors).length > 0 && (
        <div className="form-error">
          {Object.entries(fieldErrors).map(([field, message]) => (
            <div key={field}>{message}</div>
          ))}
        </div>
      )}
      {error && <div className="form-error">{error}</div>}
    </Wizard>
  );
}

export default CreateKnowledgeWizard;
