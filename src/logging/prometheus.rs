use crate::app::AppUpdate;
use crate::errors::AppError;
use crate::heart_rate::HeartRateStatus;
use crate::settings::PrometheusSettings;

use chrono::{DateTime, Local};
use http::{header, HeaderName, HeaderValue};
use log::*;
use prometheus::proto::MetricFamily;
use reqwest::Client;
use std::collections::BTreeMap;
use std::str::FromStr;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::Receiver as BReceiver;
use tokio_util::sync::CancellationToken;

use prometheus::{Encoder, IntGauge, Opts, Registry, TextEncoder};

pub(super) struct PrometheusLoggingActor {
    settings: PrometheusSettings,
    last_rr: Duration,
    activity: u8,
    built_url: String,
    registry: Registry,
    gauges: BTreeMap<String, IntGauge>,
    client: Client,
}

impl PrometheusLoggingActor {
    pub(super) fn build(
        initial_activity: u8,
        settings: PrometheusSettings,
    ) -> Result<Option<Self>, AppError> {
        let built_url = {
            let mut url = if settings.url.contains("://") {
                settings.url.to_owned()
            } else {
                format!("http://{}", settings.url)
            };
            if url.ends_with('/') {
                url.pop();
            }
            url
        };
        let registry = Registry::new();
        let mut gauges = BTreeMap::new();

        let metrics = [
            (&settings.metrics.bpm, "Heart Rate in Beats per Minute"),
            (
                &settings.metrics.rr,
                "Time between heart beats in milliseconds",
            ),
            (
                &settings.metrics.battery,
                "Battery level of Heart Rate Monitor",
            ),
            (
                &settings.metrics.twitch_up,
                "If this heart rate update triggered a TwitchUp",
            ),
            (
                &settings.metrics.twitch_down,
                "If this heart rate update triggered a TwitchDown",
            ),
            (&settings.metrics.activity, "Current index of Activity"),
        ];

        for (name, desc) in metrics.iter() {
            if !name.is_empty() {
                try_add_gauge(name, desc, &registry, &mut gauges)?;
            }
        }

        // No metrics were added! Let's close the thread early
        if gauges.is_empty() {
            return Ok(None);
        }

        let mut headers = header::HeaderMap::new();

        if !settings.header.is_empty() {
            let (header_key, header_value) = settings
                .header
                .split_once(':')
                .ok_or(AppError::MissingDelimiter)
                .map(|(k, v)| (k.trim(), v.trim()))?;
            headers.insert(
                HeaderName::from_str(header_key)?,
                HeaderValue::from_str(header_value)?,
            );
        }

        let client = Client::builder().default_headers(headers).build()?;

        Ok(Some(Self {
            settings,
            last_rr: Duration::from_secs(0),
            activity: initial_activity,
            built_url,
            registry,
            gauges,
            client,
        }))
    }

    pub(super) async fn rx_loop(
        &mut self,
        broadcast_rx: &mut BReceiver<AppUpdate>,
        cancel_token: CancellationToken,
    ) -> Result<(), AppError> {
        loop {
            tokio::select! {
                heart_rate_status = broadcast_rx.recv() => {
                    match heart_rate_status {
                        Ok(AppUpdate::HeartRateStatus(data)) => {
                            self.handle_data(data).await?;
                        },
                        Ok(AppUpdate::ActivitySelected(index)) => {
                            self.activity = index;
                        },
                        Ok(_) => {},
                        Err(RecvError::Closed) => {
                            error!("Prometheus Logging: Channel closed");
                            return Ok(());
                        },
                        Err(RecvError::Lagged(count)) => {
                            warn!("Prometheus Logging: Lagged! Missed {count} messages");
                        }
                    }
                }
                _ = cancel_token.cancelled() => {
                    info!("Logging thread shutting down");
                    return Ok(());
                }
            }
        }
    }
    fn build_buffer(&self, timestamp: &DateTime<Local>) -> Result<Vec<u8>, AppError> {
        // Gather the metrics.
        let mut buffer = vec![];
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        // Since there's not any timestamp support on the actual gauges in the Prometheus crate,
        // we're manually appending the timestamps before sending the data out
        // Taken from: https://github.com/tikv/rust-prometheus/issues/423#issuecomment-957742142
        let metric_families: Vec<MetricFamily> = metric_families
            .into_iter()
            .map(|mut fam| {
                for metric in fam.mut_metric() {
                    metric.set_timestamp_ms(timestamp.timestamp_millis())
                }
                fam
            })
            .collect();
        encoder.encode(&metric_families, &mut buffer)?;

        Ok(buffer)
    }
    async fn handle_data(&mut self, heart_rate_status: HeartRateStatus) -> Result<(), AppError> {
        if heart_rate_status.heart_rate_bpm == 0 {
            return Ok(());
        }
        let reported_rr = heart_rate_status
            .rr_intervals
            .last()
            .unwrap_or(&self.last_rr);

        let metrics = [
            (
                &self.settings.metrics.bpm,
                heart_rate_status.heart_rate_bpm as i64,
            ),
            (&self.settings.metrics.rr, reported_rr.as_millis() as i64),
            (
                &self.settings.metrics.battery,
                u8::from(heart_rate_status.battery_level) as i64,
            ),
            (
                &self.settings.metrics.twitch_up,
                heart_rate_status.twitch_up as i64,
            ),
            (
                &self.settings.metrics.twitch_down,
                heart_rate_status.twitch_down as i64,
            ),
            (&self.settings.metrics.activity, self.activity as i64),
        ];

        for (metric_name, value) in metrics.iter() {
            if !metric_name.is_empty() {
                self.gauges
                    .get(*metric_name)
                    .ok_or(AppError::MissingMetric)?
                    .set(*value);
            }
        }

        let buf = self.build_buffer(&heart_rate_status.timestamp)?;

        // Just putting errors in the .log, shutting down the whole app
        // if a webserver wasn't reachable once seems overkill.
        match self.client.post(&self.built_url).body(buf).send().await {
            Ok(_) => {}
            Err(e) => {
                error!("Error POSTing Prometheus data! {e}");
            }
        }

        self.last_rr = *reported_rr;

        Ok(())
    }
}

fn try_add_gauge(
    metric_name: &str,
    metric_desc: &str,
    registry: &Registry,
    map: &mut BTreeMap<String, IntGauge>,
) -> Result<(), AppError> {
    let opts = Opts::new(metric_name, metric_desc);
    let gauge = IntGauge::with_opts(opts)?;
    registry.register(Box::new(gauge.clone()))?;
    map.insert(metric_name.to_owned(), gauge);

    Ok(())
}
