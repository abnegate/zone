//! Hybrid media-generation intent classification.
//!
//! High-confidence rules route immediately. Anything leftover — including
//! informal edits of an attached photo that the word lists miss — is decided
//! by a short LiteLLM call (workspace Fast, the current chat model, or a small
//! installed completion model) with a small token budget. Timeouts and empty
//! hosts fall back to chat.

use serde_json::Value;
use std::time::Duration;
use zone_core::llm::{LlmClient, LlmConfig, Message};

use crate::config::ComfyUiConfig;
use crate::services::media_source::Kind as MediaKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleDecision {
    Image,
    Video,
    Audio,
    Upscale,
    Chat,
    Ambiguous,
}

/// The three lanes the short LiteLLM call can pick between. Anything it cannot
/// be read as — a refusal, prose, a timeout — is `Chat`, so an unavailable
/// classifier never starts a generation job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AmbiguousVerdict {
    Image,
    Audio,
    Chat,
}

impl AmbiguousVerdict {
    const IMAGE: &'static str = "IMAGE";
    const AUDIO: &'static str = "AUDIO";

    fn parse(answer: &str) -> Self {
        let answer = answer.trim();
        if answer.eq_ignore_ascii_case(Self::IMAGE) {
            Self::Image
        } else if answer.eq_ignore_ascii_case(Self::AUDIO) {
            Self::Audio
        } else {
            Self::Chat
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationIntent {
    Chat,
    Image,
    Video,
    Audio,
    Upscale,
}

impl GenerationIntent {
    /// Audio and upscaling mirror images, not video. A direct "generate a song"
    /// is one ComfyUI job in an agent chat exactly as "generate an image" is,
    /// and upscaling has nothing to compose with either way. The
    /// `generate_audio` / `generate_image` tools exist for the composite turns
    /// where the model decides to produce media mid-task. Video has no tool at
    /// all, so a video request falls back to chat rather than being replaced by
    /// a job the agent cannot then build on.
    pub fn yielding_to_agent(self, agent_enabled: bool) -> Self {
        match self {
            Self::Video if agent_enabled => Self::Chat,
            other => other,
        }
    }
}

#[derive(Clone)]
pub struct ImageIntentClassifier {
    config: ComfyUiConfig,
    litellm_host: String,
    litellm_key: String,
}

impl ImageIntentClassifier {
    pub fn new(config: ComfyUiConfig, litellm_host: String, litellm_key: String) -> Self {
        Self {
            config,
            litellm_host,
            litellm_key,
        }
    }

    /// Classify a message. Any unavailable, timed-out, or malformed model result
    /// safely falls back to normal chat.
    pub async fn classify(&self, content: &str, metadata: Option<&Value>) -> GenerationIntent {
        if !self.config.enabled {
            return GenerationIntent::Chat;
        }
        let flag = |name: &str| metadata.and_then(|m| m.get(name)).and_then(Value::as_bool);
        if flag("upscale") == Some(true) {
            return GenerationIntent::Upscale;
        }
        if flag("video_generation") == Some(true) {
            return GenerationIntent::Video;
        }
        if flag("audio_generation") == Some(true) {
            return GenerationIntent::Audio;
        }
        if flag("image_generation") == Some(true) {
            return GenerationIntent::Image;
        }
        let skip_video = flag("video_generation") == Some(false);
        let skip_audio = flag("audio_generation") == Some(false);
        let skip_image = flag("image_generation") == Some(false);
        let skip_upscale = flag("upscale") == Some(false);
        // Turning every generator off means "produce no media", which upscaling
        // would violate; `upscale: false` on its own only suppresses this path.
        if skip_video && skip_audio && skip_image {
            return GenerationIntent::Chat;
        }

        let has_source_image = crate::services::media_source::has_image_attachment(metadata);
        let has_source_media = crate::services::media_source::has_media_attachment(metadata);
        match deterministic_decision(content, has_source_image, has_source_media) {
            RuleDecision::Upscale if !skip_upscale => GenerationIntent::Upscale,
            RuleDecision::Video if !skip_video => GenerationIntent::Video,
            RuleDecision::Audio if !skip_audio => GenerationIntent::Audio,
            RuleDecision::Image if !skip_image => GenerationIntent::Image,
            RuleDecision::Ambiguous if !skip_image || !skip_audio => {
                match self.classify_ambiguous(content, has_source_image).await {
                    AmbiguousVerdict::Image if !skip_image => GenerationIntent::Image,
                    AmbiguousVerdict::Audio if !skip_audio => GenerationIntent::Audio,
                    _ => GenerationIntent::Chat,
                }
            }
            _ => GenerationIntent::Chat,
        }
    }

    pub async fn is_image_request(&self, content: &str, metadata: Option<&Value>) -> bool {
        self.classify(content, metadata).await == GenerationIntent::Image
    }

    async fn classify_ambiguous(&self, content: &str, has_source_image: bool) -> AmbiguousVerdict {
        if self.litellm_host.trim().is_empty() {
            return AmbiguousVerdict::Chat;
        }
        let client = LlmClient::new(LlmConfig {
            base_url: self.litellm_host.clone(),
            api_key: self.litellm_key.clone(),
            default_model: self.config.classifier_model.clone(),
            temperature: 0.0,
            max_tokens: 3,
        });
        let prompt = if has_source_image {
            format!(
                "Return exactly IMAGE, AUDIO, or CHAT. IMAGE when the user wants a new image \
                 generated now, or wants the attached image edited now: add, remove, replace, \
                 restyle, transform, change the background or environment, place the subject in a \
                 different setting, or any other change to the photo, including short or informal \
                 wording. AUDIO when the user wants a sound, a song, or a piece of music produced \
                 now. Greetings, thanks, opinions, discussion, analysis, prompt-writing, coding, \
                 and questions about the attached image are CHAT, and so is writing code, tests, \
                 documentation, prose, lyrics, or a plan about music or audio.\nUser: {content}"
            )
        } else {
            format!(
                "Return exactly IMAGE, AUDIO, or CHAT. IMAGE when the user wants a new image \
                 generated now, or wants something added to, removed from, or changed on an \
                 existing image now, including a new background or environment, even if the \
                 wording is informal. AUDIO when the user wants a sound, a song, or a piece of \
                 music produced now. Discussion, analysis, prompt-writing, coding, and how-to \
                 questions are CHAT, and so is writing code, tests, documentation, prose, lyrics, \
                 or a plan about music or audio.\nUser: {content}"
            )
        };
        let messages = [Message::user(prompt)];
        let result = tokio::time::timeout(
            Duration::from_secs(self.config.classifier_timeout_secs),
            client.chat_with_model(&self.config.classifier_model, &messages, None),
        )
        .await;

        let Ok(Ok(response)) = result else {
            return AmbiguousVerdict::Chat;
        };
        let Some(answer) = response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
        else {
            return AmbiguousVerdict::Chat;
        };
        AmbiguousVerdict::parse(answer)
    }

    /// Turn an attached-image edit request into a CLIP prompt for img2img.
    /// Falls back to a heuristic if the classifier model is unavailable.
    pub async fn edit_prompt(&self, content: &str) -> String {
        let fallback = heuristic_edit_prompt(content);
        if self.litellm_host.trim().is_empty() {
            return fallback;
        }
        let client = LlmClient::new(LlmConfig {
            base_url: self.litellm_host.clone(),
            api_key: self.litellm_key.clone(),
            default_model: self.config.classifier_model.clone(),
            temperature: 0.2,
            max_tokens: 160,
        });
        let prompt = format!(
            "Rewrite the user's request as a positive prompt for an image model that starts from \
             the attached photo. Describe the finished photograph, not the editing instruction. \
             Keep the same main subject, identity, and pose unless the user asked to change them. \
             If they asked to remove something, describe the scene without it and with that area \
             filled in naturally; do not name the removed thing. If they asked to change the \
             environment or background, describe the same subject in that new setting. \
             No quotes, labels, or preamble. One or two sentences.\nUser: {content}"
        );
        let messages = [Message::user(prompt)];
        let result = tokio::time::timeout(
            Duration::from_secs(self.config.classifier_timeout_secs),
            client.chat_with_model(&self.config.classifier_model, &messages, None),
        )
        .await;
        let Ok(Ok(response)) = result else {
            return fallback;
        };
        let Some(answer) = response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
        else {
            return fallback;
        };
        sanitize_rewritten_prompt(answer, content).unwrap_or(fallback)
    }
}

const IMAGE_ACTIONS: &[&str] = &[
    "generate",
    "create",
    "make",
    "draw",
    "render",
    "paint",
    "illustrate",
    "sketch",
];

const VISUAL_NOUNS: &[&str] = &[
    "image",
    "images",
    "picture",
    "pictures",
    "photo",
    "photos",
    "artwork",
    "illustration",
    "illustrations",
    "poster",
    "posters",
    "logo",
    "logos",
    "wallpaper",
    "wallpapers",
    "portrait",
    "portraits",
];
const VISUAL_IMPERATIVES: &[&str] = &["draw", "paint", "illustrate", "sketch"];

const VIDEO_NOUNS: &[&str] = &[
    "video",
    "videos",
    "clip",
    "clips",
    "animation",
    "animations",
    "footage",
];

/// True when the words ask for a picture that does not exist yet, which is what
/// separates "make a 4k wallpaper" from "make this 4k".
fn asks_for_a_new_image(tokens: &[String]) -> bool {
    names_after_action(tokens, VISUAL_NOUNS, false)
}

/// The same across pictures and clips, except that a noun the request points
/// back at names something that already exists: "make a 4k video" wants a new
/// clip, "make this video 4k" wants the one already here, enlarged.
fn asks_for_new_media(tokens: &[String]) -> bool {
    names_after_action(tokens, VISUAL_NOUNS, true) || names_after_action(tokens, VIDEO_NOUNS, true)
}

fn names_after_action(tokens: &[String], nouns: &[&str], only_new: bool) -> bool {
    tokens.iter().enumerate().any(|(index, token)| {
        IMAGE_ACTIONS.contains(&token.as_str())
            && tokens
                .iter()
                .enumerate()
                .skip(index + 1)
                .take(7)
                .any(|(at, candidate)| {
                    nouns.contains(&candidate.as_str()) && !(only_new && points_back(tokens, at))
                })
    })
}

fn points_back(tokens: &[String], noun: usize) -> bool {
    noun > 0
        && matches!(
            tokens[noun - 1].as_str(),
            "this" | "that" | "these" | "those" | "the" | "its" | "my" | "your"
        )
}

fn deterministic_decision(
    content: &str,
    has_source_image: bool,
    has_source_media: bool,
) -> RuleDecision {
    let tokens = tokenize(content);
    if tokens.is_empty() {
        return RuleDecision::Chat;
    }
    let has = |word: &str| tokens.iter().any(|token| token == word);
    let has_phrase = |phrase: &[&str]| phrase_in(&tokens, phrase);

    // High-confidence non-generation intents take precedence. Exact tokens
    // avoid treating "a teacher explaining relativity" as a request to
    // explain image generation.
    let asks_how = has_phrase(&["how", "to"]) || has_phrase(&["how", "do", "i"]);
    let programming_language = ["react", "typescript", "javascript", "rust", "python"]
        .iter()
        .any(|word| has(word));
    let implementation_term = [
        "component",
        "function",
        "class",
        "api",
        "workflow",
        "code",
        "implement",
        "html",
        "css",
    ]
    .iter()
    .any(|word| has(word));
    let software_only_noun = ["endpoint", "endpoints"].iter().any(|word| has(word));
    let asks_for_visual = tokens.iter().any(|token| {
        VISUAL_NOUNS.contains(&token.as_str()) || VISUAL_IMPERATIVES.contains(&token.as_str())
    });
    let discusses_code = (software_only_noun && !asks_for_visual)
        || (programming_language && implementation_term)
        || ((has("image") || has("images"))
            && ["component", "api", "workflow", "code", "implement"]
                .iter()
                .any(|word| has(word)));
    let analysis_request = ["describe", "analyze", "analyse", "inspect", "look"]
        .iter()
        .any(|word| has(word))
        && (has("image") || has("picture") || has("photo"));
    let prompt_request =
        has("prompt") && (has("write") || has("improve") || has_phrase(&["prompt", "for"]));
    let discussion_request = has_phrase(&["talk", "about"])
        || has_phrase(&["discuss", "image"])
        || (asks_how
            && (has("generate") || has("create") || has("make") || has("add") || has("remove")));
    if analysis_request || prompt_request || discussion_request || discusses_code || asks_how {
        return RuleDecision::Chat;
    }

    if is_upscale_request(&tokens, &has_phrase, has_source_media) {
        return RuleDecision::Upscale;
    }

    if is_video_request(&tokens, &has_phrase) {
        return RuleDecision::Video;
    }

    let audio = audio_signal(&tokens, &has_phrase);
    if audio == AudioSignal::Certain {
        return RuleDecision::Audio;
    }

    if is_edit_request(&tokens, &has_phrase)
        && (has_source_image || refers_to_existing_image(&tokens, &has_phrase))
    {
        return RuleDecision::Image;
    }

    let explicit = asks_for_a_new_image(&tokens);
    let visual_imperative = tokens
        .iter()
        .take(4)
        .any(|token| VISUAL_IMPERATIVES.contains(&token.as_str()))
        && !["conclusion", "conclusions", "attention", "parallel"]
            .iter()
            .any(|word| has(word));
    if explicit || visual_imperative {
        return RuleDecision::Image;
    }

    if audio == AudioSignal::Possible {
        return RuleDecision::Ambiguous;
    }

    if tokens.iter().any(|token| {
        IMAGE_ACTIONS.contains(&token.as_str()) || VISUAL_NOUNS.contains(&token.as_str())
    }) || has("visualize")
        || has("visualise")
        || has_source_image
    {
        // Attached photos: let the fast classifier catch informal edits the
        // word lists miss ("same subject at night", "without the chair").
        RuleDecision::Ambiguous
    } else {
        RuleDecision::Chat
    }
}

/// True when the prompt asks to change an image that is already on the thread.
pub fn should_reuse_thread_image(content: &str) -> bool {
    let tokens = tokenize(content);
    let has_phrase = |phrase: &[&str]| phrase_in(&tokens, phrase);
    match deterministic_decision(content, false, false) {
        RuleDecision::Upscale => true,
        RuleDecision::Image | RuleDecision::Video => {
            refers_to_existing_image(&tokens, &has_phrase)
                || is_animate_existing(&tokens, &has_phrase)
        }
        _ => false,
    }
}

/// The media an upscale request names, when it names one. The first noun wins,
/// so "the screenshot from the video" is about the screenshot. `None` leaves the
/// choice to whatever the thread offers most recently.
pub fn upscale_target(content: &str) -> Option<MediaKind> {
    const IMAGE_NOUNS: &[&str] = &[
        "image",
        "images",
        "picture",
        "pictures",
        "photo",
        "photos",
        "photograph",
        "photographs",
        "screenshot",
        "screenshots",
    ];
    tokenize(content).iter().find_map(|token| {
        if VIDEO_NOUNS.contains(&token.as_str()) {
            Some(MediaKind::Video)
        } else if IMAGE_NOUNS.contains(&token.as_str()) {
            Some(MediaKind::Image)
        } else {
            None
        }
    })
}

fn tokenize(content: &str) -> Vec<String> {
    content
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn phrase_in(tokens: &[String], phrase: &[&str]) -> bool {
    phrase_end(tokens, phrase).is_some()
}

fn phrase_end(tokens: &[String], phrase: &[&str]) -> Option<usize> {
    tokens
        .windows(phrase.len())
        .position(|window| window.iter().map(String::as_str).eq(phrase.iter().copied()))
        .map(|start| start + phrase.len() - 1)
}

/// Upscaling is the one media request that cannot invent its subject, so a
/// resolution word is never enough on its own. Either the request names the act
/// ("upscale this", "hi-res version"), or it pairs an enlarging verb with a
/// resolution and introduces nothing new — "make this 4k" enlarges, while
/// "make it a 4k wallpaper" and "make a 4k video" are asking for a new picture
/// and a new clip.
fn is_upscale_request(
    tokens: &[String],
    has_phrase: &impl Fn(&[&str]) -> bool,
    has_source_media: bool,
) -> bool {
    let has = |word: &str| tokens.iter().any(|token| token == word);
    let verb = [
        "upscale",
        "upscales",
        "upscaled",
        "upscaling",
        "upres",
        "upsize",
        "upsample",
        "upsampled",
        "upsampling",
    ]
    .iter()
    .any(|word| has(word));
    let named = verb
        || has("hires")
        || has("highres")
        || has_phrase(&["hi", "res"])
        || has_phrase(&["high", "res"])
        || has_phrase(&["super", "resolution"]);

    let resolution = ["4k", "8k", "1080p", "1440p", "2160p", "4320p", "uhd"]
        .iter()
        .any(|word| has(word))
        || ((has("resolution") || has("res") || has("dpi") || has("pixels"))
            && [
                "increase",
                "increased",
                "raise",
                "boost",
                "improve",
                "enhance",
                "double",
                "quadruple",
                "higher",
                "high",
                "bigger",
                "larger",
                "more",
                "better",
                "full",
            ]
            .iter()
            .any(|word| has(word)));
    let bigger = resolution
        && ENLARGE_ACTIONS.iter().any(|word| has(word))
        && !produces_something_new(tokens);

    if !named && !bigger {
        return false;
    }
    // Only naming the act outright survives a request for something new, so
    // "make it a hi-res poster" is still a poster.
    if !verb && produces_something_new(tokens) {
        return false;
    }
    // "is this 4k", "what resolution is the photo" — asking about a picture is
    // not asking for a bigger one. An enlarging verb makes it a request again.
    if !bigger && asks_about_media(tokens) {
        return false;
    }
    names_existing_media(tokens, has_phrase)
        || ["it", "this", "that", "these", "those", "them"]
            .iter()
            .any(|word| has(word))
        || has_source_media
}

const ENLARGE_ACTIONS: &[&str] = &[
    "make",
    "get",
    "render",
    "convert",
    "bump",
    "scale",
    "resize",
    "enlarge",
    "expand",
    "upgrade",
    "increase",
    "raise",
    "boost",
    "improve",
    "enhance",
    "double",
    "quadruple",
];

/// An article after the verb introduces something that does not exist yet, which
/// separates "make this 4k" from "make it a 4k wallpaper".
fn produces_something_new(tokens: &[String]) -> bool {
    let Some(verb) = tokens.iter().position(|token| {
        ENLARGE_ACTIONS.contains(&token.as_str()) || IMAGE_ACTIONS.contains(&token.as_str())
    }) else {
        return false;
    };
    tokens
        .iter()
        .skip(verb + 1)
        .take(7)
        .any(|token| token == "a" || token == "an")
        || asks_for_new_media(tokens)
}

fn asks_about_media(tokens: &[String]) -> bool {
    tokens.iter().take(3).any(|token| {
        matches!(
            token.as_str(),
            "what"
                | "whats"
                | "why"
                | "when"
                | "where"
                | "which"
                | "who"
                | "is"
                | "are"
                | "was"
                | "were"
                | "do"
                | "does"
                | "did"
        )
    })
}

fn names_existing_media(tokens: &[String], has_phrase: &impl Fn(&[&str]) -> bool) -> bool {
    refers_to_existing_image(tokens, has_phrase)
        || has_phrase(&["this", "video"])
        || has_phrase(&["the", "video"])
        || has_phrase(&["that", "video"])
        || has_phrase(&["this", "clip"])
        || has_phrase(&["the", "clip"])
        || has_phrase(&["that", "clip"])
        || has_phrase(&["this", "animation"])
        || has_phrase(&["the", "animation"])
        || has_phrase(&["this", "footage"])
        || has_phrase(&["the", "footage"])
        || has_phrase(&["this", "one"])
        || has_phrase(&["of", "this"])
        || has_phrase(&["of", "it"])
}

fn is_video_request(tokens: &[String], has_phrase: &impl Fn(&[&str]) -> bool) -> bool {
    const ACTIONS: &[&str] = &["generate", "create", "make", "render", "animate"];
    let animate = tokens
        .iter()
        .take(4)
        .any(|token| token == "animate" || token == "animating");
    let explicit = tokens.iter().enumerate().any(|(index, token)| {
        ACTIONS.contains(&token.as_str())
            && tokens
                .iter()
                .skip(index + 1)
                .take(7)
                .any(|candidate| VIDEO_NOUNS.contains(&candidate.as_str()))
    });
    explicit
        || animate
        || has_phrase(&["text", "to", "video"])
        || has_phrase(&["image", "to", "video"])
        || has_phrase(&["make", "this", "move"])
        || has_phrase(&["make", "it", "move"])
        || has_phrase(&["make", "this", "a", "video"])
        || has_phrase(&["turn", "this", "into", "a", "video"])
        || has_phrase(&["bring", "this", "to", "life"])
}

/// Words that close a direct object: after one of these the noun phrase the
/// verb governs has ended, so a later audio noun belongs to something else
/// ("make a playlist *of* songs" is a list, not a song).
const OBJECT_BREAKS: &[&str] = &[
    "about", "after", "and", "around", "as", "at", "because", "before", "but", "by", "during",
    "for", "from", "if", "in", "into", "like", "of", "on", "once", "or", "over", "since", "so",
    "than", "that", "then", "through", "to", "until", "when", "where", "which", "while", "with",
    "without",
];

/// Closed-class words that can follow the head of a noun phrase without
/// modifying it. A content word that is not one of these is a second noun, so
/// the audio word in front of it was attributive: "music *player*", "song
/// *lyrics*", "audio *pipeline*", "music *API*".
const PHRASE_TRAILERS: &[&str] = &[
    "a", "again", "an", "instead", "now", "only", "please", "the", "thanks", "this", "today", "too",
];

const OBJECT_WINDOW: usize = 7;

/// Audio words that name a thing only an audio model produces, so a request to
/// make one is never a request for text or code.
const AUDIO_OBJECTS: &[&str] = &[
    "audio",
    "beat",
    "beats",
    "instrumental",
    "instrumentals",
    "jingle",
    "jingles",
    "melodies",
    "melody",
    "music",
    "sfx",
    "song",
    "songs",
    "soundtrack",
    "soundtracks",
    "track",
    "tracks",
    "tune",
    "tunes",
];

/// Audio words that just as readily name a document, a control-flow construct,
/// or a design token. They only ever raise a question for the classifier.
const AUDIO_TOPICS: &[&str] = &[
    "ambiance", "ambience", "loop", "loops", "lyrics", "mp3", "score", "scores", "sounds", "theme",
    "themes",
];

const VISUAL_VETO: &[&str] = &[
    "art",
    "banner",
    "banners",
    "cover",
    "covers",
    "flyer",
    "flyers",
    "graphic",
    "graphics",
    "sleeve",
    "thumbnail",
    "thumbnails",
    "visual",
    "visuals",
];

/// How strongly a message asks for audio. `Certain` is terminal; `Possible`
/// goes to the classifier, which is the only thing that can tell "write a song
/// about the sea" from "write a poem about music".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioSignal {
    None,
    Possible,
    Certain,
}

/// True when one of `nouns` is the head of the noun phrase `action` governs:
/// reached without crossing a preposition, and not itself modifying a later
/// noun.
fn governs_object(tokens: &[String], action: usize, nouns: &[&str]) -> bool {
    tokens
        .iter()
        .enumerate()
        .skip(action + 1)
        .take(OBJECT_WINDOW)
        .take_while(|(_, token)| !OBJECT_BREAKS.contains(&token.as_str()))
        .any(|(index, token)| {
            nouns.contains(&token.as_str())
                && tokens.get(index + 1).is_none_or(|next| {
                    OBJECT_BREAKS.contains(&next.as_str())
                        || PHRASE_TRAILERS.contains(&next.as_str())
                })
        })
}

fn audio_signal(tokens: &[String], has_phrase: &impl Fn(&[&str]) -> bool) -> AudioSignal {
    const ACTIONS: &[&str] = &["compose", "create", "generate", "make", "produce"];
    const DRAFTS: &[&str] = &["write", "writing"];
    const REQUESTS: &[&[&str]] = &[
        &["give", "me"],
        &["i", "want"],
        &["i", "need"],
        &["i", "d", "like"],
        &["i", "would", "like"],
    ];

    if tokens.iter().any(|token| {
        VISUAL_NOUNS.contains(&token.as_str()) || VISUAL_VETO.contains(&token.as_str())
    }) {
        return AudioSignal::None;
    }

    let acting = tokens.iter().any(|token| ACTIONS.contains(&token.as_str()));
    if has_phrase(&["text", "to", "audio"])
        || has_phrase(&["text", "to", "music"])
        || (acting && (has_phrase(&["sound", "effect"]) || has_phrase(&["sound", "effects"])))
    {
        return AudioSignal::Certain;
    }

    let commanded = |verbs: &[&str]| {
        tokens.iter().enumerate().any(|(index, token)| {
            verbs.contains(&token.as_str()) && governs_object(tokens, index, AUDIO_OBJECTS)
        })
    };
    if commanded(ACTIONS) {
        return AudioSignal::Certain;
    }

    let requested = REQUESTS
        .iter()
        .filter_map(|frame| phrase_end(tokens, frame))
        .any(|index| governs_object(tokens, index, AUDIO_OBJECTS));
    let scented = tokens.iter().any(|token| {
        AUDIO_TOPICS.contains(&token.as_str()) || AUDIO_OBJECTS.contains(&token.as_str())
    });
    if commanded(DRAFTS) || requested || (acting && scented) {
        AudioSignal::Possible
    } else {
        AudioSignal::None
    }
}

fn is_animate_existing(tokens: &[String], has_phrase: &impl Fn(&[&str]) -> bool) -> bool {
    has_phrase(&["animate", "this"])
        || has_phrase(&["animate", "it"])
        || has_phrase(&["make", "this", "move"])
        || has_phrase(&["make", "it", "move"])
        || has_phrase(&["make", "this", "a", "video"])
        || has_phrase(&["turn", "this", "into", "a", "video"])
        || (tokens.iter().any(|token| token == "animate")
            && refers_to_existing_image(tokens, has_phrase))
}

fn is_edit_request(tokens: &[String], has_phrase: &impl Fn(&[&str]) -> bool) -> bool {
    const STRONG_EDITS: &[&str] = &[
        "add",
        "remove",
        "delete",
        "erase",
        "replace",
        "insert",
        "overlay",
        "crop",
        "inpaint",
        "outpaint",
        "recolor",
        "restyle",
        "remix",
        "redraw",
        "repaint",
        "reimagine",
        "edit",
        "transform",
        "modify",
        "convert",
        "wipe",
        "relocate",
    ];
    const WEAK_EDITS: &[&str] = &[
        "put", "place", "take", "fill", "hide", "fix", "clean", "clear", "brighten", "darken",
        "sharpen", "blur", "vary", "swap", "move",
    ];
    let weak_edit = tokens
        .iter()
        .any(|token| WEAK_EDITS.contains(&token.as_str()));
    tokens
        .iter()
        .any(|token| STRONG_EDITS.contains(&token.as_str()))
        || (weak_edit && refers_to_existing_image(tokens, has_phrase))
        || has_phrase(&["get", "rid"])
        || has_phrase(&["take", "out"])
        || has_phrase(&["take", "off"])
        || has_phrase(&["cut", "out"])
        || has_phrase(&["make", "this"])
        || has_phrase(&["make", "it"])
        || has_phrase(&["turn", "this"])
        || has_phrase(&["turn", "it"])
        || has_phrase(&["change", "this"])
        || has_phrase(&["change", "the"])
        || has_phrase(&["put", "this"])
        || has_phrase(&["place", "this"])
        || has_phrase(&["put", "it"])
        || has_phrase(&["place", "it"])
        || has_phrase(&["move", "this"])
        || has_phrase(&["different", "environment"])
        || has_phrase(&["new", "environment"])
        || has_phrase(&["another", "environment"])
        || has_phrase(&["different", "background"])
        || has_phrase(&["new", "background"])
        || has_phrase(&["different", "setting"])
        || has_phrase(&["new", "setting"])
        || has_phrase(&["another", "scene"])
        || has_phrase(&["based", "on", "this"])
        || has_phrase(&["from", "this"])
        || has_phrase(&["using", "this"])
        || has_phrase(&["to", "this"])
        || has_phrase(&["in", "this"])
        || has_phrase(&["on", "this"])
        || has_phrase(&["into", "this"])
}

fn refers_to_existing_image(_tokens: &[String], has_phrase: &impl Fn(&[&str]) -> bool) -> bool {
    has_phrase(&["this", "image"])
        || has_phrase(&["this", "picture"])
        || has_phrase(&["this", "photo"])
        || has_phrase(&["the", "image"])
        || has_phrase(&["the", "picture"])
        || has_phrase(&["the", "photo"])
        || has_phrase(&["that", "image"])
        || has_phrase(&["that", "picture"])
        || has_phrase(&["that", "photo"])
        || has_phrase(&["the", "attached"])
        || has_phrase(&["this", "object"])
        || has_phrase(&["the", "object"])
        || has_phrase(&["this", "subject"])
        || has_phrase(&["this", "one"])
        || has_phrase(&["from", "an", "image"])
        || has_phrase(&["from", "this"])
        || has_phrase(&["put", "this"])
        || has_phrase(&["place", "this"])
        || has_phrase(&["put", "it"])
        || has_phrase(&["place", "it"])
        || has_phrase(&["this", "background"])
        || has_phrase(&["the", "background"])
        || has_phrase(&["this", "watermark"])
        || has_phrase(&["the", "watermark"])
}

fn is_removal_request(tokens: &[String], has_phrase: &impl Fn(&[&str]) -> bool) -> bool {
    ["remove", "delete", "erase", "wipe"]
        .iter()
        .any(|word| tokens.iter().any(|token| token == *word))
        || has_phrase(&["get", "rid"])
        || has_phrase(&["take", "out"])
        || has_phrase(&["take", "off"])
        || has_phrase(&["cut", "out"])
}

fn is_environment_change(tokens: &[String], has_phrase: &impl Fn(&[&str]) -> bool) -> bool {
    tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "environment" | "background" | "setting" | "scene" | "backdrop"
        )
    }) || has_phrase(&["put", "this"])
        || has_phrase(&["place", "this"])
        || has_phrase(&["put", "it"])
        || has_phrase(&["place", "it"])
        || has_phrase(&["in", "a"])
        || has_phrase(&["into", "a"])
}

/// CLIP text for img2img when the classifier model cannot rewrite the request.
pub fn heuristic_edit_prompt(content: &str) -> String {
    let tokens = tokenize(content);
    let has_phrase = |phrase: &[&str]| phrase_in(&tokens, phrase);
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return "the same subject, edited as requested, photorealistic".to_string();
    }
    if is_removal_request(&tokens, &has_phrase) {
        format!(
            "the same photograph with the requested object gone, that area filled in naturally \
             to match the surrounding scene, no leftover object or hole, photorealistic. {trimmed}"
        )
    } else if is_environment_change(&tokens, &has_phrase) {
        format!(
            "the same subject in the new environment described, keep the subject's identity, \
             pose, and appearance, only change the setting, matching lighting, photorealistic. \
             {trimmed}"
        )
    } else {
        format!(
            "the same subject with the requested edits applied, keep identity and composition \
             unless asked to change them, photorealistic. {trimmed}"
        )
    }
}

fn sanitize_rewritten_prompt(answer: &str, original: &str) -> Option<String> {
    let mut text = answer.trim().trim_matches('"').trim_matches('\'').trim();
    if let Some(stripped) = text.strip_prefix("Prompt:") {
        text = stripped.trim();
    }
    if text.is_empty()
        || text.len() > 4_000
        || text.eq_ignore_ascii_case("IMAGE")
        || text.eq_ignore_ascii_case("CHAT")
        || text.eq_ignore_ascii_case("VIDEO")
    {
        return None;
    }
    if text.eq_ignore_ascii_case(original.trim()) {
        return Some(heuristic_edit_prompt(original));
    }
    Some(format!("{text} {}", original.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    /// An attached image is attached media too, so the two source flags the
    /// rules read only ever disagree when a clip is the attachment.
    fn decide(content: &str, has_source_image: bool) -> RuleDecision {
        deterministic_decision(content, has_source_image, has_source_image)
    }

    fn decide_with_video(content: &str) -> RuleDecision {
        deterministic_decision(content, false, true)
    }

    #[test]
    fn agent_mode_keeps_images_and_yields_video_to_chat() {
        assert_eq!(
            GenerationIntent::Image.yielding_to_agent(true),
            GenerationIntent::Image
        );
        assert_eq!(
            GenerationIntent::Video.yielding_to_agent(true),
            GenerationIntent::Chat
        );
        assert_eq!(
            GenerationIntent::Video.yielding_to_agent(false),
            GenerationIntent::Video
        );
        assert_eq!(
            GenerationIntent::Chat.yielding_to_agent(true),
            GenerationIntent::Chat
        );
    }

    #[test]
    fn agent_mode_serves_audio_directly_like_images() {
        assert_eq!(
            GenerationIntent::Audio.yielding_to_agent(true),
            GenerationIntent::Audio
        );
        assert_eq!(
            GenerationIntent::Audio.yielding_to_agent(false),
            GenerationIntent::Audio
        );
    }

    #[test]
    fn audio_requests_do_not_steal_image_requests() {
        for image in [
            "generate an image of a music studio",
            "make a poster for a music festival",
            "generate an image of a beat-up truck",
            "create an image with noise texture",
        ] {
            assert_eq!(decide(image, false), RuleDecision::Image, "{image}");
        }
        assert_eq!(
            decide("make a music video of a fox", false),
            RuleDecision::Video
        );
        for not_audio in [
            "create an album cover for my song",
            "write a rust function that plays audio",
            "make a music video of a fox",
            "keep track of the noise levels",
        ] {
            assert_ne!(decide(not_audio, false), RuleDecision::Audio, "{not_audio}");
        }
    }

    #[test]
    fn audio_rules_leave_code_and_writing_alone() {
        for not_audio in [
            "write a function that plays audio",
            "write a go function that plays audio",
            "write a music player component",
            "create an audio recording endpoint",
            "write tests for the audio pipeline",
            "write documentation for the music API",
            "write song lyrics about the sea",
            "make a playlist of songs for a party",
            "write a blog post about music production",
            "create a music theory lesson",
            "write a poem about music",
            "make a loop over the array",
        ] {
            assert_ne!(decide(not_audio, false), RuleDecision::Audio, "{not_audio}");
        }

        for chat in [
            "write a function that plays audio",
            "write a go function that plays audio",
            "write a music player component",
            "write tests for the audio pipeline",
            "write documentation for the music API",
            "write song lyrics about the sea",
            "write a blog post about music production",
            "write a poem about music",
            "create an audio recording endpoint",
            "create audio recording endpoints",
            "create an endpoint that returns song metadata",
            "create a rust endpoint that streams music",
        ] {
            assert_eq!(decide(chat, false), RuleDecision::Chat, "{chat}");
        }
    }

    #[test]
    fn implementation_nouns_leave_the_generation_lanes_alone() {
        for image in [
            "generate an image of a music studio",
            "make a poster for a music festival",
            "draw a line segment with two endpoints",
            "draw a diagram showing the endpoints of a vector",
            "generate an image of the endpoint of a hiking trail",
            "illustrate the endpoint of the journey",
            "generate an image of a rust covered endpoint",
        ] {
            assert_eq!(decide(image, false), RuleDecision::Image, "{image}");
        }

        assert_eq!(
            decide("make a music video of a fox", false),
            RuleDecision::Video,
        );
    }

    #[test]
    fn audio_rule_matrix() {
        for audio in [
            "make a background audio track that sounds like shuffling through a forest",
            "Generate a song about the sea",
            "Create some music for my podcast",
            "Make me a jingle",
            "compose a melody in a minor key",
            "generate audio of rain falling",
            "produce an instrumental for the intro",
            "text to music of a piano piece",
            "make a sound effect of a door closing",
            "make a background track that sounds like shuffling through a forest",
            "make me some sfx of a door creaking",
            "make a beat for the chorus",
            "text to audio of a cat purring",
        ] {
            assert_eq!(decide(audio, false), RuleDecision::Audio, "{audio}");
        }
    }

    #[test]
    fn softer_audio_phrasings_reach_the_classifier() {
        for ambiguous in [
            "generate ambient rain sounds",
            "generate a 30 second loop of ocean waves",
            "generate an mp3 of birdsong",
            "give me a song about the sea",
            "I want a song about the sea",
            "write a song about the sea",
            "create a music theory lesson",
            "make a playlist of songs for a party",
        ] {
            assert_eq!(
                decide(ambiguous, false),
                RuleDecision::Ambiguous,
                "{ambiguous}"
            );
        }
    }

    #[test]
    fn classifier_rule_matrix() {
        for request in [
            "Generate an image of a red panda",
            "Generate an image of the same rooster facing the other way",
            "please draw a picture of the moon",
            "Create a picture of our city",
            "Illustrate a quiet forest",
            "Generate an image of a teacher explaining relativity",
            "Make me three images of red pandas",
            "Create pictures showing the four seasons",
            "Generate an image of a Python snake",
            "generate images",
            "make me an image",
        ] {
            assert_eq!(decide(request, false), RuleDecision::Image, "{request}");
        }
        for video in [
            "Generate a video of a red panda",
            "Create a clip of waves crashing",
            "Make me a video of a lighthouse",
            "Animate this image",
            "Turn this into a video",
            "image to video of this photo",
            "text to video of a fox running",
            "Make this move",
        ] {
            assert_eq!(
                decide(video, video.contains("this") || video.contains("photo")),
                RuleDecision::Video,
                "{video}"
            );
        }
        for edit in [
            "Make this a watercolor",
            "Turn this into a painting",
            "Edit this image to add a sunset",
            "Restyle this as cyberpunk",
            "Change the background to a forest",
            "Remove from this image",
            "Add to this image",
            "Add a hat to this picture",
            "Remove the person from this photo",
            "Delete the text in this image",
            "Erase the watermark",
            "Put a sunset in this photo",
            "Take the logo off this image",
            "Get rid of the background",
            "Replace the sky in this picture",
            "Put this object in a different environment",
            "Remove this object from an image",
            "Place this on a beach",
            "Put this in a snowy forest",
        ] {
            assert_eq!(decide(edit, true), RuleDecision::Image, "{edit}");
        }
        for needs_source in [
            "Make this a watercolor",
            "Turn this into a painting",
            "Restyle this as cyberpunk",
        ] {
            assert_ne!(
                decide(needs_source, false),
                RuleDecision::Image,
                "{needs_source}"
            );
        }
        for named_image in [
            "Remove from this image",
            "Add to this image",
            "Add a hat to this picture",
            "Edit this image to add a sunset",
            "Change the background to a forest",
            "Put this object in a different environment",
            "Remove this object from an image",
            "Place this on a beach",
        ] {
            assert_eq!(
                decide(named_image, false),
                RuleDecision::Image,
                "{named_image}"
            );
            assert!(should_reuse_thread_image(named_image), "{named_image}");
        }
        assert!(!should_reuse_thread_image(
            "Generate an image of a blue fox"
        ));
        assert!(!should_reuse_thread_image(
            "Generate an image of the environment"
        ));
        assert!(!should_reuse_thread_image(
            "Generate an image of a wolf with a snowy background"
        ));
        assert!(should_reuse_thread_image(
            "Change the background to a forest"
        ));
        assert_eq!(
            decide("please take a look, can you fix it?", true),
            RuleDecision::Ambiguous
        );
        assert_eq!(
            decide("please take a look, can you fix it?", false),
            RuleDecision::Chat
        );
        assert!(should_reuse_thread_image("Animate this image"));
        assert!(should_reuse_thread_image("Make this a video"));
        assert!(!should_reuse_thread_image("Generate a video of a fox"));
        assert!(!should_reuse_thread_image(
            "How do I add a hat to this image?"
        ));
        assert_eq!(
            decide("How do I add a hat to this image?", true),
            RuleDecision::Chat
        );
        assert_eq!(decide("Add a hat", false), RuleDecision::Chat);
        for chat in [
            "Explain image generation code",
            "Write an image prompt for a red panda",
            "Describe this image",
            "How do I generate an image with Rust?",
            "How should I create a React image component?",
            "Discuss image components in React",
            "Render an image component in React",
            "What is the capital of France?",
        ] {
            assert_eq!(decide(chat, false), RuleDecision::Chat, "{chat}");
        }
        assert_eq!(decide("Describe this image", true), RuleDecision::Chat);
        assert_eq!(
            decide("Could you design a logo for Acme?", false),
            RuleDecision::Ambiguous
        );
        assert_eq!(
            decide("the same subject at night", false),
            RuleDecision::Chat
        );
        assert_eq!(
            decide("the same subject at night", true),
            RuleDecision::Ambiguous
        );
        assert_eq!(decide("without the chair", true), RuleDecision::Ambiguous);
        assert_eq!(decide("thanks", true), RuleDecision::Ambiguous);
    }

    #[test]
    fn upscale_rule_matrix() {
        for request in [
            "upscale this",
            "Upscale this image",
            "upscale the photo please",
            "can you upres this",
            "upsample this picture",
            "make this 4k",
            "make it 8k",
            "render this at 1080p",
            "make this photo higher resolution",
            "increase the resolution of the image",
            "enhance the resolution on this one",
            "give me a hi-res version of this",
            "super resolution on the attached shot",
        ] {
            assert_eq!(decide(request, true), RuleDecision::Upscale, "{request}");
        }

        for request in [
            "upscale this video",
            "upscale the clip",
            "make this video 4k",
            "bump the footage to 1080p",
        ] {
            assert_eq!(
                decide_with_video(request),
                RuleDecision::Upscale,
                "{request}"
            );
        }
    }

    #[test]
    fn naming_a_resolution_never_steals_a_request_for_new_media() {
        // A clip is still a clip, even at a named resolution, and even with a
        // photo attached to the turn.
        for request in [
            "make a 4k video of a sunset",
            "generate an 8k video of a red panda",
            "create a 1080p clip of waves",
            "make a video of this at 1080p",
            "animate this to 4k",
            "render 4k video",
        ] {
            assert_eq!(decide(request, true), RuleDecision::Video, "{request}");
        }
        // A new picture wins with nothing attached and nothing on the thread,
        // whichever way the request is phrased.
        for request in [
            "make it a 4k wallpaper of a mountain",
            "make this a 4k wallpaper of a mountain",
            "make it a high res logo of a fox",
            "make it a hi-res poster of a wolf",
        ] {
            assert_eq!(decide(request, false), RuleDecision::Image, "{request}");
        }
        // Editing an attached photo is still an edit.
        for request in [
            "Change the background to a 4k mountain vista",
            "Put this in a 4k forest scene",
            "Make this a watercolor at 4k",
        ] {
            assert_eq!(decide(request, true), RuleDecision::Image, "{request}");
        }
        // Talking about resolution is not asking for more of it, wherever the
        // question word falls.
        for request in [
            "hey is this 4k",
            "my monitor is 4k, does this look right",
            "so is this photo high res",
            "tell me about 8k tvs",
            "write a blog post about 4k monitors",
            "summarise this 1080p spec sheet",
        ] {
            assert_ne!(decide(request, true), RuleDecision::Upscale, "{request}");
        }
        // Pointing at a clip that already exists still upscales it.
        assert_eq!(decide("make this video 4k", true), RuleDecision::Upscale);
    }

    #[test]
    fn a_resolution_word_alone_does_not_upscale() {
        // Asking for a new picture wins even when a source image is attached
        // and the request names a resolution.
        for request in [
            "generate a 4k wallpaper of a mountain",
            "create a high resolution image of a cat",
            "draw a 1080p poster for the show",
        ] {
            assert_ne!(decide(request, true), RuleDecision::Upscale, "{request}");
        }
        // Nothing to upscale: no attachment and no reference to the thread.
        for request in [
            "what does 4k mean",
            "how do I increase the resolution of a photo",
        ] {
            assert_ne!(decide(request, false), RuleDecision::Upscale, "{request}");
        }
        // Asking about a picture is not asking for a bigger one, even with one
        // attached.
        for request in [
            "is this 4k",
            "what resolution is this photo",
            "does the image have enough pixels for print",
        ] {
            assert_ne!(decide(request, true), RuleDecision::Upscale, "{request}");
        }
        // A polite imperative is still a request.
        assert_eq!(decide("can you make this 4k", true), RuleDecision::Upscale);
        // A plain edit stays an edit.
        assert_eq!(decide("add a hat to this photo", true), RuleDecision::Image);
    }

    #[test]
    fn upscaling_always_reuses_thread_media() {
        assert!(should_reuse_thread_image("upscale it"));
        assert!(should_reuse_thread_image("make this 4k"));
        assert!(should_reuse_thread_image("upscale the video"));
        assert!(!should_reuse_thread_image(
            "generate a 4k wallpaper of a mountain"
        ));
        assert!(!should_reuse_thread_image("what does upscaling mean"));
    }

    #[test]
    fn upscale_target_follows_the_noun() {
        assert_eq!(upscale_target("upscale this video"), Some(MediaKind::Video));
        assert_eq!(upscale_target("upscale the clip"), Some(MediaKind::Video));
        assert_eq!(upscale_target("upscale this photo"), Some(MediaKind::Image));
        assert_eq!(upscale_target("upscale this"), None);
        assert_eq!(
            upscale_target("upscale the screenshot from the video"),
            Some(MediaKind::Image)
        );
    }

    #[tokio::test]
    async fn upscale_metadata_flag_forces_and_skips() {
        let config = ComfyUiConfig {
            enabled: true,
            ..Default::default()
        };
        let classifier = ImageIntentClassifier::new(config, String::new(), String::new());
        assert_eq!(
            classifier
                .classify("hello", Some(&serde_json::json!({"upscale": true})))
                .await,
            GenerationIntent::Upscale
        );
        assert_ne!(
            classifier
                .classify("upscale this", Some(&serde_json::json!({"upscale": false})))
                .await,
            GenerationIntent::Upscale
        );
        assert_eq!(
            classifier
                .classify(
                    "upscale this",
                    Some(&serde_json::json!({
                        "image_generation": false,
                        "video_generation": false
                    }))
                )
                .await,
            GenerationIntent::Chat
        );
    }

    #[test]
    fn upscaling_survives_agent_mode() {
        assert_eq!(
            GenerationIntent::Upscale.yielding_to_agent(true),
            GenerationIntent::Upscale
        );
    }

    #[tokio::test]
    async fn metadata_override_and_disabled_guard() {
        let mut config = ComfyUiConfig {
            enabled: true,
            ..Default::default()
        };
        let classifier = ImageIntentClassifier::new(config.clone(), String::new(), String::new());
        assert!(
            classifier
                .is_image_request(
                    "hello",
                    Some(&serde_json::json!({"image_generation": true}))
                )
                .await
        );
        assert!(
            !classifier
                .is_image_request(
                    "generate an image",
                    Some(&serde_json::json!({"image_generation": false}))
                )
                .await
        );

        config.enabled = false;
        let disabled = ImageIntentClassifier::new(config, String::new(), String::new());
        assert!(
            !disabled
                .is_image_request(
                    "generate an image",
                    Some(&serde_json::json!({"image_generation": true}))
                )
                .await
        );
    }

    #[tokio::test]
    async fn audio_metadata_flag_forces_and_skips_audio() {
        let classifier = ImageIntentClassifier::new(
            ComfyUiConfig {
                enabled: true,
                ..Default::default()
            },
            String::new(),
            String::new(),
        );
        assert_eq!(
            classifier
                .classify(
                    "hello",
                    Some(&serde_json::json!({"audio_generation": true}))
                )
                .await,
            GenerationIntent::Audio
        );
        assert_eq!(
            classifier
                .classify(
                    "Generate a song about the sea",
                    Some(&serde_json::json!({"audio_generation": false}))
                )
                .await,
            GenerationIntent::Chat
        );
    }

    async fn classifier_answering(answer: &str) -> (MockServer, ImageIntentClassifier) {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "classification",
                "object": "chat.completion",
                "created": 0,
                "model": "fast",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": answer},
                    "finish_reason": "stop"
                }]
            })))
            .mount(&server)
            .await;
        let classifier = ImageIntentClassifier::new(
            ComfyUiConfig {
                enabled: true,
                classifier_model: "fast".to_string(),
                ..Default::default()
            },
            server.uri(),
            "key".to_string(),
        );
        (server, classifier)
    }

    #[tokio::test]
    async fn ambiguous_arm_answers_audio_image_or_chat() {
        const SOFT_AUDIO: &str = "generate ambient rain sounds";

        let (_audio, classifier) = classifier_answering(AmbiguousVerdict::AUDIO).await;
        assert_eq!(
            classifier.classify(SOFT_AUDIO, None).await,
            GenerationIntent::Audio
        );
        for recovered in [
            "generate a 30 second loop of ocean waves",
            "generate an mp3 of birdsong",
            "give me a song about the sea",
            "I want a song about the sea",
        ] {
            assert_eq!(
                classifier.classify(recovered, None).await,
                GenerationIntent::Audio,
                "{recovered}"
            );
        }

        let (_image, classifier) = classifier_answering(AmbiguousVerdict::IMAGE).await;
        assert_eq!(
            classifier.classify(SOFT_AUDIO, None).await,
            GenerationIntent::Image
        );

        let (_chat, classifier) = classifier_answering("maybe some audio?").await;
        assert_eq!(
            classifier.classify(SOFT_AUDIO, None).await,
            GenerationIntent::Chat
        );

        let hostless = ImageIntentClassifier::new(
            ComfyUiConfig {
                enabled: true,
                ..Default::default()
            },
            String::new(),
            String::new(),
        );
        assert_eq!(
            hostless.classify(SOFT_AUDIO, None).await,
            GenerationIntent::Chat
        );
    }

    #[tokio::test]
    async fn ambiguous_audio_respects_the_skip_audio_flag() {
        let (_server, classifier) = classifier_answering(AmbiguousVerdict::AUDIO).await;
        assert_eq!(
            classifier
                .classify(
                    "generate ambient rain sounds",
                    Some(&serde_json::json!({"audio_generation": false}))
                )
                .await,
            GenerationIntent::Chat
        );
    }

    #[tokio::test]
    async fn ambiguous_uses_strict_litellm_binary_result() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "classification",
                "object": "chat.completion",
                "created": 0,
                "model": "fast",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "IMAGE"},
                    "finish_reason": "stop"
                }]
            })))
            .mount(&server)
            .await;
        let classifier = ImageIntentClassifier::new(
            ComfyUiConfig {
                enabled: true,
                classifier_model: "fast".to_string(),
                ..Default::default()
            },
            server.uri(),
            "key".to_string(),
        );
        assert!(
            classifier
                .is_image_request("Design a logo for Acme", None)
                .await
        );
    }

    #[tokio::test]
    async fn malformed_classifier_response_falls_back_to_chat() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "classification",
                "object": "chat.completion",
                "created": 0,
                "model": "fast",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "perhaps image"},
                    "finish_reason": "stop"
                }]
            })))
            .mount(&server)
            .await;
        let classifier = ImageIntentClassifier::new(
            ComfyUiConfig {
                enabled: true,
                ..Default::default()
            },
            server.uri(),
            "key".to_string(),
        );
        assert!(
            !classifier
                .is_image_request("Design a logo for Acme", None)
                .await
        );
    }

    #[tokio::test]
    async fn classifier_timeout_falls_back_to_chat() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(2))
                    .set_body_json(serde_json::json!({})),
            )
            .mount(&server)
            .await;
        let classifier = ImageIntentClassifier::new(
            ComfyUiConfig {
                enabled: true,
                classifier_timeout_secs: 1,
                ..Default::default()
            },
            server.uri(),
            "key".to_string(),
        );
        assert!(
            !classifier
                .is_image_request("Design a logo for Acme", None)
                .await
        );
    }

    #[test]
    fn heuristic_edit_prompt_rewrites_environment_and_removal() {
        let environment = heuristic_edit_prompt("Put this object in a different environment");
        assert!(environment.contains("new environment"));
        assert!(environment.contains("Put this object in a different environment"));

        let beach = heuristic_edit_prompt("Place this on a beach");
        assert!(beach.contains("new environment"));
        assert!(beach.contains("Place this on a beach"));

        let removal = heuristic_edit_prompt("Remove this object from an image");
        assert!(removal.contains("requested object gone"));
        assert!(removal.contains("Remove this object from an image"));

        let style = heuristic_edit_prompt("Make this a watercolor");
        assert!(style.contains("requested edits"));
        assert!(style.contains("Make this a watercolor"));
    }

    #[test]
    fn sanitize_rewritten_prompt_keeps_original_instruction() {
        let rewritten = sanitize_rewritten_prompt(
            "A wooden chair on a misty forest path, photorealistic.",
            "Put this object in a different environment",
        )
        .unwrap();
        assert!(rewritten.contains("wooden chair on a misty forest path"));
        assert!(rewritten.contains("Put this object in a different environment"));

        assert!(sanitize_rewritten_prompt("IMAGE", "remove the chair").is_none());
        let echoed = sanitize_rewritten_prompt(
            "Remove this object from an image",
            "Remove this object from an image",
        )
        .unwrap();
        assert!(echoed.contains("requested object gone"));
    }

    #[tokio::test]
    async fn edit_prompt_uses_rewritten_scene_and_keeps_original() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "rewrite",
                "object": "chat.completion",
                "created": 0,
                "model": "fast",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "A wooden chair on a misty forest path, photorealistic."
                    },
                    "finish_reason": "stop"
                }]
            })))
            .mount(&server)
            .await;
        let classifier = ImageIntentClassifier::new(
            ComfyUiConfig {
                enabled: true,
                classifier_model: "fast".to_string(),
                ..Default::default()
            },
            server.uri(),
            "key".to_string(),
        );
        let prompt = classifier
            .edit_prompt("Put this object in a different environment")
            .await;
        assert!(prompt.contains("wooden chair on a misty forest path"));
        assert!(prompt.contains("Put this object in a different environment"));
    }

    #[tokio::test]
    async fn edit_prompt_falls_back_when_host_is_empty() {
        let classifier = ImageIntentClassifier::new(
            ComfyUiConfig {
                enabled: true,
                ..Default::default()
            },
            String::new(),
            String::new(),
        );
        let prompt = classifier
            .edit_prompt("Remove this object from an image")
            .await;
        assert!(prompt.contains("requested object gone"));
        assert!(prompt.contains("Remove this object from an image"));
    }

    fn attached_png() -> serde_json::Value {
        serde_json::json!({
            "attachments": [{
                "name": "photo.png",
                "mime": "image/png",
                "url": "data:image/png;base64,aaa"
            }]
        })
    }

    #[tokio::test]
    async fn attached_informal_edit_uses_fast_classifier() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "classification",
                "object": "chat.completion",
                "created": 0,
                "model": "fast",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "IMAGE"},
                    "finish_reason": "stop"
                }]
            })))
            .mount(&server)
            .await;
        let classifier = ImageIntentClassifier::new(
            ComfyUiConfig {
                enabled: true,
                classifier_model: "fast".to_string(),
                ..Default::default()
            },
            server.uri(),
            "key".to_string(),
        );
        assert!(
            classifier
                .is_image_request("the same subject at night", Some(&attached_png()))
                .await
        );
    }

    #[tokio::test]
    async fn attached_thanks_stays_chat_when_classifier_says_chat() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "classification",
                "object": "chat.completion",
                "created": 0,
                "model": "fast",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "CHAT"},
                    "finish_reason": "stop"
                }]
            })))
            .mount(&server)
            .await;
        let classifier = ImageIntentClassifier::new(
            ComfyUiConfig {
                enabled: true,
                classifier_model: "fast".to_string(),
                ..Default::default()
            },
            server.uri(),
            "key".to_string(),
        );
        assert!(
            !classifier
                .is_image_request("thanks", Some(&attached_png()))
                .await
        );
    }

    #[tokio::test]
    async fn attached_informal_edit_stays_chat_without_classifier_host() {
        let classifier = ImageIntentClassifier::new(
            ComfyUiConfig {
                enabled: true,
                ..Default::default()
            },
            String::new(),
            String::new(),
        );
        assert!(
            !classifier
                .is_image_request("the same subject at night", Some(&attached_png()))
                .await
        );
    }
}
