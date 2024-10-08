use core::fmt;
use std::{borrow::Cow, sync::Arc};

use opentelemetry::{
    global,
    metrics::{
        noop::NoopAsyncInstrument, Callback, Counter, Gauge, Histogram, InstrumentProvider,
        MetricsError, ObservableCounter, ObservableGauge, ObservableUpDownCounter, Result,
        UpDownCounter,
    },
};

use crate::instrumentation::Scope;
use crate::metrics::{
    instrument::{Instrument, InstrumentKind, Observable, ResolvedMeasures},
    internal::{self, Number},
    pipeline::{Pipelines, Resolver},
};

// maximum length of instrument name
const INSTRUMENT_NAME_MAX_LENGTH: usize = 255;
// maximum length of instrument unit name
const INSTRUMENT_UNIT_NAME_MAX_LENGTH: usize = 63;
const INSTRUMENT_NAME_ALLOWED_NON_ALPHANUMERIC_CHARS: [char; 4] = ['_', '.', '-', '/'];

// instrument validation error strings
const INSTRUMENT_NAME_EMPTY: &str = "instrument name must be non-empty";
const INSTRUMENT_NAME_LENGTH: &str = "instrument name must be less than 256 characters";
const INSTRUMENT_NAME_INVALID_CHAR: &str =
    "characters in instrument name must be ASCII and belong to the alphanumeric characters, '_', '.', '-' and '/'";
const INSTRUMENT_NAME_FIRST_ALPHABETIC: &str =
    "instrument name must start with an alphabetic character";
const INSTRUMENT_UNIT_LENGTH: &str = "instrument unit must be less than 64 characters";
const INSTRUMENT_UNIT_INVALID_CHAR: &str = "characters in instrument unit must be ASCII";

/// Handles the creation and coordination of all metric instruments.
///
/// A meter represents a single instrumentation scope; all metric telemetry
/// produced by an instrumentation scope will use metric instruments from a
/// single meter.
///
/// See the [Meter API] docs for usage.
///
/// [Meter API]: opentelemetry::metrics::Meter
pub struct SdkMeter {
    scope: Scope,
    pipes: Arc<Pipelines>,
    u64_resolver: Resolver<u64>,
    i64_resolver: Resolver<i64>,
    f64_resolver: Resolver<f64>,
    validation_policy: InstrumentValidationPolicy,
}

impl SdkMeter {
    pub(crate) fn new(scope: Scope, pipes: Arc<Pipelines>) -> Self {
        let view_cache = Default::default();

        SdkMeter {
            scope,
            pipes: Arc::clone(&pipes),
            u64_resolver: Resolver::new(Arc::clone(&pipes), Arc::clone(&view_cache)),
            i64_resolver: Resolver::new(Arc::clone(&pipes), Arc::clone(&view_cache)),
            f64_resolver: Resolver::new(pipes, view_cache),
            validation_policy: InstrumentValidationPolicy::HandleGlobalAndIgnore,
        }
    }

    #[cfg(test)]
    fn with_validation_policy(self, validation_policy: InstrumentValidationPolicy) -> Self {
        Self {
            validation_policy,
            ..self
        }
    }
}

#[doc(hidden)]
impl InstrumentProvider for SdkMeter {
    fn u64_counter(
        &self,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
    ) -> Result<Counter<u64>> {
        validate_instrument_config(name.as_ref(), &unit, self.validation_policy)?;
        let p = InstrumentResolver::new(self, &self.u64_resolver);
        p.lookup(InstrumentKind::Counter, name, description, unit)
            .map(|i| Counter::new(Arc::new(i)))
    }

    fn f64_counter(
        &self,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
    ) -> Result<Counter<f64>> {
        validate_instrument_config(name.as_ref(), &unit, self.validation_policy)?;
        let p = InstrumentResolver::new(self, &self.f64_resolver);
        p.lookup(InstrumentKind::Counter, name, description, unit)
            .map(|i| Counter::new(Arc::new(i)))
    }

    fn u64_observable_counter(
        &self,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
        callbacks: Vec<Callback<u64>>,
    ) -> Result<ObservableCounter<u64>> {
        validate_instrument_config(name.as_ref(), &unit, self.validation_policy)?;
        let p = InstrumentResolver::new(self, &self.u64_resolver);
        let ms = p.measures(
            InstrumentKind::ObservableCounter,
            name.clone(),
            description.clone(),
            unit.clone(),
        )?;
        if ms.is_empty() {
            return Ok(ObservableCounter::new(Arc::new(NoopAsyncInstrument::new())));
        }

        let observable = Arc::new(Observable::new(ms));

        for callback in callbacks {
            let cb_inst = Arc::clone(&observable);
            self.pipes
                .register_callback(move || callback(cb_inst.as_ref()));
        }

        Ok(ObservableCounter::new(observable))
    }

    fn f64_observable_counter(
        &self,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
        callbacks: Vec<Callback<f64>>,
    ) -> Result<ObservableCounter<f64>> {
        validate_instrument_config(name.as_ref(), &unit, self.validation_policy)?;
        let p = InstrumentResolver::new(self, &self.f64_resolver);
        let ms = p.measures(
            InstrumentKind::ObservableCounter,
            name.clone(),
            description.clone(),
            unit.clone(),
        )?;
        if ms.is_empty() {
            return Ok(ObservableCounter::new(Arc::new(NoopAsyncInstrument::new())));
        }
        let observable = Arc::new(Observable::new(ms));

        for callback in callbacks {
            let cb_inst = Arc::clone(&observable);
            self.pipes
                .register_callback(move || callback(cb_inst.as_ref()));
        }

        Ok(ObservableCounter::new(observable))
    }

    fn i64_up_down_counter(
        &self,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
    ) -> Result<UpDownCounter<i64>> {
        validate_instrument_config(name.as_ref(), &unit, self.validation_policy)?;
        let p = InstrumentResolver::new(self, &self.i64_resolver);
        p.lookup(InstrumentKind::UpDownCounter, name, description, unit)
            .map(|i| UpDownCounter::new(Arc::new(i)))
    }

    fn f64_up_down_counter(
        &self,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
    ) -> Result<UpDownCounter<f64>> {
        validate_instrument_config(name.as_ref(), &unit, self.validation_policy)?;
        let p = InstrumentResolver::new(self, &self.f64_resolver);
        p.lookup(InstrumentKind::UpDownCounter, name, description, unit)
            .map(|i| UpDownCounter::new(Arc::new(i)))
    }

    fn i64_observable_up_down_counter(
        &self,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
        callbacks: Vec<Callback<i64>>,
    ) -> Result<ObservableUpDownCounter<i64>> {
        validate_instrument_config(name.as_ref(), &unit, self.validation_policy)?;
        let p = InstrumentResolver::new(self, &self.i64_resolver);
        let ms = p.measures(
            InstrumentKind::ObservableUpDownCounter,
            name.clone(),
            description.clone(),
            unit.clone(),
        )?;
        if ms.is_empty() {
            return Ok(ObservableUpDownCounter::new(Arc::new(
                NoopAsyncInstrument::new(),
            )));
        }

        let observable = Arc::new(Observable::new(ms));

        for callback in callbacks {
            let cb_inst = Arc::clone(&observable);
            self.pipes
                .register_callback(move || callback(cb_inst.as_ref()));
        }

        Ok(ObservableUpDownCounter::new(observable))
    }

    fn f64_observable_up_down_counter(
        &self,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
        callbacks: Vec<Callback<f64>>,
    ) -> Result<ObservableUpDownCounter<f64>> {
        validate_instrument_config(name.as_ref(), &unit, self.validation_policy)?;
        let p = InstrumentResolver::new(self, &self.f64_resolver);
        let ms = p.measures(
            InstrumentKind::ObservableUpDownCounter,
            name.clone(),
            description.clone(),
            unit.clone(),
        )?;
        if ms.is_empty() {
            return Ok(ObservableUpDownCounter::new(Arc::new(
                NoopAsyncInstrument::new(),
            )));
        }

        let observable = Arc::new(Observable::new(ms));

        for callback in callbacks {
            let cb_inst = Arc::clone(&observable);
            self.pipes
                .register_callback(move || callback(cb_inst.as_ref()));
        }

        Ok(ObservableUpDownCounter::new(observable))
    }

    fn u64_gauge(
        &self,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
    ) -> Result<Gauge<u64>> {
        validate_instrument_config(name.as_ref(), &unit, self.validation_policy)?;
        let p = InstrumentResolver::new(self, &self.u64_resolver);
        p.lookup(InstrumentKind::Gauge, name, description, unit)
            .map(|i| Gauge::new(Arc::new(i)))
    }

    fn f64_gauge(
        &self,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
    ) -> Result<Gauge<f64>> {
        validate_instrument_config(name.as_ref(), &unit, self.validation_policy)?;
        let p = InstrumentResolver::new(self, &self.f64_resolver);
        p.lookup(InstrumentKind::Gauge, name, description, unit)
            .map(|i| Gauge::new(Arc::new(i)))
    }

    fn i64_gauge(
        &self,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
    ) -> Result<Gauge<i64>> {
        validate_instrument_config(name.as_ref(), &unit, self.validation_policy)?;
        let p = InstrumentResolver::new(self, &self.i64_resolver);
        p.lookup(InstrumentKind::Gauge, name, description, unit)
            .map(|i| Gauge::new(Arc::new(i)))
    }

    fn u64_observable_gauge(
        &self,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
        callbacks: Vec<Callback<u64>>,
    ) -> Result<ObservableGauge<u64>> {
        validate_instrument_config(name.as_ref(), &unit, self.validation_policy)?;
        let p = InstrumentResolver::new(self, &self.u64_resolver);
        let ms = p.measures(
            InstrumentKind::ObservableGauge,
            name.clone(),
            description.clone(),
            unit.clone(),
        )?;
        if ms.is_empty() {
            return Ok(ObservableGauge::new(Arc::new(NoopAsyncInstrument::new())));
        }

        let observable = Arc::new(Observable::new(ms));

        for callback in callbacks {
            let cb_inst = Arc::clone(&observable);
            self.pipes
                .register_callback(move || callback(cb_inst.as_ref()));
        }

        Ok(ObservableGauge::new(observable))
    }

    fn i64_observable_gauge(
        &self,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
        callbacks: Vec<Callback<i64>>,
    ) -> Result<ObservableGauge<i64>> {
        validate_instrument_config(name.as_ref(), &unit, self.validation_policy)?;
        let p = InstrumentResolver::new(self, &self.i64_resolver);
        let ms = p.measures(
            InstrumentKind::ObservableGauge,
            name.clone(),
            description.clone(),
            unit.clone(),
        )?;
        if ms.is_empty() {
            return Ok(ObservableGauge::new(Arc::new(NoopAsyncInstrument::new())));
        }

        let observable = Arc::new(Observable::new(ms));

        for callback in callbacks {
            let cb_inst = Arc::clone(&observable);
            self.pipes
                .register_callback(move || callback(cb_inst.as_ref()));
        }

        Ok(ObservableGauge::new(observable))
    }

    fn f64_observable_gauge(
        &self,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
        callbacks: Vec<Callback<f64>>,
    ) -> Result<ObservableGauge<f64>> {
        validate_instrument_config(name.as_ref(), &unit, self.validation_policy)?;
        let p = InstrumentResolver::new(self, &self.f64_resolver);
        let ms = p.measures(
            InstrumentKind::ObservableGauge,
            name.clone(),
            description.clone(),
            unit.clone(),
        )?;
        if ms.is_empty() {
            return Ok(ObservableGauge::new(Arc::new(NoopAsyncInstrument::new())));
        }

        let observable = Arc::new(Observable::new(ms));

        for callback in callbacks {
            let cb_inst = Arc::clone(&observable);
            self.pipes
                .register_callback(move || callback(cb_inst.as_ref()));
        }

        Ok(ObservableGauge::new(observable))
    }

    fn f64_histogram(
        &self,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
    ) -> Result<Histogram<f64>> {
        validate_instrument_config(name.as_ref(), &unit, self.validation_policy)?;
        let p = InstrumentResolver::new(self, &self.f64_resolver);
        p.lookup(InstrumentKind::Histogram, name, description, unit)
            .map(|i| Histogram::new(Arc::new(i)))
    }

    fn u64_histogram(
        &self,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
    ) -> Result<Histogram<u64>> {
        validate_instrument_config(name.as_ref(), &unit, self.validation_policy)?;
        let p = InstrumentResolver::new(self, &self.u64_resolver);
        p.lookup(InstrumentKind::Histogram, name, description, unit)
            .map(|i| Histogram::new(Arc::new(i)))
    }
}

/// Validation policy for instrument
#[derive(Clone, Copy)]
enum InstrumentValidationPolicy {
    HandleGlobalAndIgnore,
    /// Currently only for test
    #[cfg(test)]
    Strict,
}

fn validate_instrument_config(
    name: &str,
    unit: &Option<Cow<'static, str>>,
    policy: InstrumentValidationPolicy,
) -> Result<()> {
    match validate_instrument_name(name).and_then(|_| validate_instrument_unit(unit)) {
        Ok(_) => Ok(()),
        Err(err) => match policy {
            InstrumentValidationPolicy::HandleGlobalAndIgnore => {
                global::handle_error(err);
                Ok(())
            }
            #[cfg(test)]
            InstrumentValidationPolicy::Strict => Err(err),
        },
    }
}

fn validate_instrument_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(MetricsError::InvalidInstrumentConfiguration(
            INSTRUMENT_NAME_EMPTY,
        ));
    }
    if name.len() > INSTRUMENT_NAME_MAX_LENGTH {
        return Err(MetricsError::InvalidInstrumentConfiguration(
            INSTRUMENT_NAME_LENGTH,
        ));
    }
    if name.starts_with(|c: char| !c.is_ascii_alphabetic()) {
        return Err(MetricsError::InvalidInstrumentConfiguration(
            INSTRUMENT_NAME_FIRST_ALPHABETIC,
        ));
    }
    if name.contains(|c: char| {
        !c.is_ascii_alphanumeric() && !INSTRUMENT_NAME_ALLOWED_NON_ALPHANUMERIC_CHARS.contains(&c)
    }) {
        return Err(MetricsError::InvalidInstrumentConfiguration(
            INSTRUMENT_NAME_INVALID_CHAR,
        ));
    }
    Ok(())
}

fn validate_instrument_unit(unit: &Option<Cow<'static, str>>) -> Result<()> {
    if let Some(unit) = unit {
        if unit.len() > INSTRUMENT_UNIT_NAME_MAX_LENGTH {
            return Err(MetricsError::InvalidInstrumentConfiguration(
                INSTRUMENT_UNIT_LENGTH,
            ));
        }
        if unit.contains(|c: char| !c.is_ascii()) {
            return Err(MetricsError::InvalidInstrumentConfiguration(
                INSTRUMENT_UNIT_INVALID_CHAR,
            ));
        }
    }
    Ok(())
}

impl fmt::Debug for SdkMeter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Meter").field("scope", &self.scope).finish()
    }
}

/// Provides all OpenTelemetry instruments.
struct InstrumentResolver<'a, T> {
    meter: &'a SdkMeter,
    resolve: &'a Resolver<T>,
}

impl<'a, T> InstrumentResolver<'a, T>
where
    T: Number<T>,
{
    fn new(meter: &'a SdkMeter, resolve: &'a Resolver<T>) -> Self {
        InstrumentResolver { meter, resolve }
    }

    /// lookup returns the resolved measures.
    fn lookup(
        &self,
        kind: InstrumentKind,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
    ) -> Result<ResolvedMeasures<T>> {
        let aggregators = self.measures(kind, name, description, unit)?;
        Ok(ResolvedMeasures {
            measures: aggregators,
        })
    }

    fn measures(
        &self,
        kind: InstrumentKind,
        name: Cow<'static, str>,
        description: Option<Cow<'static, str>>,
        unit: Option<Cow<'static, str>>,
    ) -> Result<Vec<Arc<dyn internal::Measure<T>>>> {
        let inst = Instrument {
            name,
            description: description.unwrap_or_default(),
            unit: unit.unwrap_or_default(),
            kind: Some(kind),
            scope: self.meter.scope.clone(),
        };

        self.resolve.measures(inst)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use opentelemetry::metrics::{InstrumentProvider, MeterProvider, MetricsError};

    use super::{
        InstrumentValidationPolicy, SdkMeter, INSTRUMENT_NAME_FIRST_ALPHABETIC,
        INSTRUMENT_NAME_INVALID_CHAR, INSTRUMENT_NAME_LENGTH, INSTRUMENT_UNIT_INVALID_CHAR,
        INSTRUMENT_UNIT_LENGTH,
    };
    use crate::{
        metrics::{pipeline::Pipelines, SdkMeterProvider},
        Resource, Scope,
    };

    #[test]
    #[ignore = "See issue https://github.com/open-telemetry/opentelemetry-rust/issues/1699"]
    fn test_instrument_creation() {
        let provider = SdkMeterProvider::builder().build();
        let meter = provider.meter("test");
        assert!(meter.u64_counter("test").try_init().is_ok());
        let result = meter.u64_counter("test with invalid name").try_init();
        // this assert fails, as result is always ok variant.
        assert!(result.is_err());
    }

    #[test]
    fn test_instrument_config_validation() {
        // scope and pipelines are not related to test
        let meter = SdkMeter::new(
            Scope::default(),
            Arc::new(Pipelines::new(Resource::default(), Vec::new(), Vec::new())),
        )
        .with_validation_policy(InstrumentValidationPolicy::Strict);
        // (name, expected error)
        let instrument_name_test_cases = vec![
            ("validateName", ""),
            ("_startWithNoneAlphabet", INSTRUMENT_NAME_FIRST_ALPHABETIC),
            ("utf8char锈", INSTRUMENT_NAME_INVALID_CHAR),
            ("a".repeat(255).leak(), ""),
            ("a".repeat(256).leak(), INSTRUMENT_NAME_LENGTH),
            ("invalid name", INSTRUMENT_NAME_INVALID_CHAR),
            ("allow/slash", ""),
            ("allow_under_score", ""),
            ("allow.dots.ok", ""),
        ];
        for (name, expected_error) in instrument_name_test_cases {
            let assert = |result: Result<_, MetricsError>| {
                if expected_error.is_empty() {
                    assert!(result.is_ok());
                } else {
                    assert!(matches!(
                        result.unwrap_err(),
                        MetricsError::InvalidInstrumentConfiguration(msg) if msg == expected_error
                    ));
                }
            };

            assert(meter.u64_counter(name.into(), None, None).map(|_| ()));
            assert(meter.f64_counter(name.into(), None, None).map(|_| ()));
            assert(
                meter
                    .u64_observable_counter(name.into(), None, None, Vec::new())
                    .map(|_| ()),
            );
            assert(
                meter
                    .f64_observable_counter(name.into(), None, None, Vec::new())
                    .map(|_| ()),
            );
            assert(
                meter
                    .i64_up_down_counter(name.into(), None, None)
                    .map(|_| ()),
            );
            assert(
                meter
                    .f64_up_down_counter(name.into(), None, None)
                    .map(|_| ()),
            );
            assert(
                meter
                    .i64_observable_up_down_counter(name.into(), None, None, Vec::new())
                    .map(|_| ()),
            );
            assert(
                meter
                    .f64_observable_up_down_counter(name.into(), None, None, Vec::new())
                    .map(|_| ()),
            );
            assert(meter.u64_gauge(name.into(), None, None).map(|_| ()));
            assert(meter.f64_gauge(name.into(), None, None).map(|_| ()));
            assert(meter.i64_gauge(name.into(), None, None).map(|_| ()));
            assert(
                meter
                    .u64_observable_gauge(name.into(), None, None, Vec::new())
                    .map(|_| ()),
            );
            assert(
                meter
                    .i64_observable_gauge(name.into(), None, None, Vec::new())
                    .map(|_| ()),
            );
            assert(
                meter
                    .f64_observable_gauge(name.into(), None, None, Vec::new())
                    .map(|_| ()),
            );
            assert(meter.f64_histogram(name.into(), None, None).map(|_| ()));
            assert(meter.u64_histogram(name.into(), None, None).map(|_| ()));
        }

        // (unit, expected error)
        let instrument_unit_test_cases = vec![
            (
                "0123456789012345678901234567890123456789012345678901234567890123",
                INSTRUMENT_UNIT_LENGTH,
            ),
            ("utf8char锈", INSTRUMENT_UNIT_INVALID_CHAR),
            ("kb", ""),
            ("Kb/sec", ""),
            ("%", ""),
            ("", ""),
        ];

        for (unit, expected_error) in instrument_unit_test_cases {
            let assert = |result: Result<_, MetricsError>| {
                if expected_error.is_empty() {
                    assert!(result.is_ok());
                } else {
                    assert!(matches!(
                        result.unwrap_err(),
                        MetricsError::InvalidInstrumentConfiguration(msg) if msg == expected_error
                    ));
                }
            };
            let unit = Some(unit.into());
            assert(
                meter
                    .u64_counter("test".into(), None, unit.clone())
                    .map(|_| ()),
            );
            assert(
                meter
                    .f64_counter("test".into(), None, unit.clone())
                    .map(|_| ()),
            );
            assert(
                meter
                    .u64_observable_counter("test".into(), None, unit.clone(), Vec::new())
                    .map(|_| ()),
            );
            assert(
                meter
                    .f64_observable_counter("test".into(), None, unit.clone(), Vec::new())
                    .map(|_| ()),
            );
            assert(
                meter
                    .i64_up_down_counter("test".into(), None, unit.clone())
                    .map(|_| ()),
            );
            assert(
                meter
                    .f64_up_down_counter("test".into(), None, unit.clone())
                    .map(|_| ()),
            );
            assert(
                meter
                    .i64_observable_up_down_counter("test".into(), None, unit.clone(), Vec::new())
                    .map(|_| ()),
            );
            assert(
                meter
                    .f64_observable_up_down_counter("test".into(), None, unit.clone(), Vec::new())
                    .map(|_| ()),
            );
            assert(
                meter
                    .u64_observable_gauge("test".into(), None, unit.clone(), Vec::new())
                    .map(|_| ()),
            );
            assert(
                meter
                    .i64_observable_gauge("test".into(), None, unit.clone(), Vec::new())
                    .map(|_| ()),
            );
            assert(
                meter
                    .f64_observable_gauge("test".into(), None, unit.clone(), Vec::new())
                    .map(|_| ()),
            );
            assert(
                meter
                    .f64_histogram("test".into(), None, unit.clone())
                    .map(|_| ()),
            );
            assert(
                meter
                    .u64_histogram("test".into(), None, unit.clone())
                    .map(|_| ()),
            );
        }
    }
}
