use std::fmt::Debug;
use std::time::Instant;

use metrics::Label;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

struct SpanSpec {
    name: &'static str,
    total: &'static str,
    duration: &'static str,
    labels: &'static [&'static str],
}

const SPANS: &[SpanSpec] = &[
    SpanSpec {
        name: "command",
        total: "bot_commands_total",
        duration: "bot_command_duration_seconds",
        labels: &["module", "command", "outcome"],
    },
    SpanSpec {
        name: "component",
        total: "bot_components_total",
        duration: "bot_component_duration_seconds",
        labels: &["module", "outcome"],
    },
    SpanSpec {
        name: "cron_job",
        total: "bot_cron_runs_total",
        duration: "bot_cron_duration_seconds",
        labels: &["job"],
    },
];

const EVENT_ERROR_LABELS: &[&str] = &["module", "event"];

pub(crate) fn layer<S>() -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    MetricsLayer
        .with_filter(Targets::new().with_target("bot_runtime", LevelFilter::INFO))
}

struct MetricsLayer;

struct SpanMetrics {
    spec: &'static SpanSpec,
    values: Vec<Option<String>>,
    start: Instant,
}

impl<S> Layer<S> for MetricsLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let name = attrs.metadata().name();
        let Some(spec) = SPANS.iter().find(|spec| spec.name == name) else {
            return;
        };
        let Some(span) = ctx.span(id) else {
            return;
        };

        let mut values = vec![None; spec.labels.len()];
        attrs.record(&mut LabelVisitor { keys: spec.labels, values: &mut values });
        span.extensions_mut().insert(SpanMetrics {
            spec,
            values,
            start: Instant::now(),
        });
    }

    fn on_record(&self, id: &Id, record: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        if let Some(metrics) = extensions.get_mut::<SpanMetrics>() {
            record.record(&mut LabelVisitor {
                keys: metrics.spec.labels,
                values: &mut metrics.values,
            });
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else {
            return;
        };
        let Some(span_metrics) = span.extensions_mut().remove::<SpanMetrics>()
        else {
            return;
        };

        let spec = span_metrics.spec;
        let labels = labels(spec.labels, &span_metrics.values);
        metrics::histogram!(spec.duration, labels.clone())
            .record(span_metrics.start.elapsed().as_secs_f64());
        metrics::counter!(spec.total, labels).increment(1);
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() != Level::ERROR {
            return;
        }

        let mut values = vec![None; EVENT_ERROR_LABELS.len()];
        event.record(&mut LabelVisitor {
            keys: EVENT_ERROR_LABELS,
            values: &mut values,
        });
        if values.iter().all(Option::is_some) {
            metrics::counter!(
                "bot_event_errors_total",
                labels(EVENT_ERROR_LABELS, &values)
            )
            .increment(1);
        }
    }
}

fn labels(keys: &'static [&'static str], values: &[Option<String>]) -> Vec<Label> {
    keys.iter()
        .zip(values)
        .map(|(key, value)| {
            Label::new(*key, value.clone().unwrap_or_else(|| "unknown".to_owned()))
        })
        .collect()
}

struct LabelVisitor<'a> {
    keys: &'static [&'static str],
    values: &'a mut [Option<String>],
}

impl LabelVisitor<'_> {
    fn set(&mut self, field: &Field, value: String) {
        if let Some(slot) = self
            .keys
            .iter()
            .position(|key| *key == field.name())
            .and_then(|i| self.values.get_mut(i))
        {
            *slot = Some(value);
        }
    }
}

impl Visit for LabelVisitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.set(field, value.to_owned());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.set(field, format!("{value:?}"));
    }
}
