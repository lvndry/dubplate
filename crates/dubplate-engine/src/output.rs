use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, FromSample, OutputCallbackInfo, SampleFormat, SizedSample, StreamConfig,
    SupportedBufferSize,
};

use crate::MAX_BLOCK_FRAMES;
use crate::controller::{Controller, offline_engine};
use crate::denormals::FlushDenormals;
use crate::error::EngineError;
use crate::mixer::Renderer;

/// How to open the audio output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutputConfig {
    /// Frames per device buffer. Smaller buffers lower the latency and give the audio thread
    /// less time per callback. `None` keeps the device's default. Values outside what the device
    /// supports are clamped to its range.
    pub buffer_frames: Option<u32>,
}

/// The fewest output channels that carry the headphones, on outputs 3 and 4.
const HEADPHONE_CHANNEL_COUNT: u16 = 4;
/// The silent output's rate and frames per buffer when the configuration leaves them open.
const SILENT_SAMPLE_RATE: u32 = 48_000;
const SILENT_BUFFER_FRAMES: u32 = 256;
/// The name the silent output goes by.
pub const SILENT_OUTPUT_NAME: &str = "Silent output";

/// An output device the engine can play on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDevice {
    /// The device's name, to pass to [`AudioOutput::start_on`].
    pub name: String,
    /// Output channels at the device's default settings. Four or more carry the headphones.
    pub channel_count: u16,
    /// The device's default sample rate in hertz.
    pub sample_rate: u32,
    /// Whether the system plays through this device by default.
    pub is_default: bool,
}

impl OutputDevice {
    /// Whether the device has outputs 3 and 4 for the headphones, like a DJ controller's sound
    /// card.
    pub fn has_headphone_output(&self) -> bool {
        self.channel_count >= HEADPHONE_CHANNEL_COUNT
    }
}

fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|description| description.name().to_owned())
        .unwrap_or_else(|_| "unnamed output".to_owned())
}

/// The sound card output. Audio plays until this is dropped.
pub struct AudioOutput {
    _sink: Sink,
    device_name: String,
    channel_count: u16,
    sample_rate: u32,
    sample_format: SampleFormat,
}

impl std::fmt::Debug for AudioOutput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AudioOutput")
            .field("device_name", &self.device_name)
            .field("channel_count", &self.channel_count)
            .field("sample_rate", &self.sample_rate)
            .field("sample_format", &self.sample_format)
            .finish_non_exhaustive()
    }
}

impl AudioOutput {
    /// Opens the default output device at its default sample rate and starts the engine on it.
    pub fn start(config: &OutputConfig) -> Result<(Controller, AudioOutput), EngineError> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or(EngineError::NoOutputDevice)?;
        Self::start_device(device, config)
    }

    /// Opens the output device called `name`, as [`AudioOutput::output_devices`] lists it, and
    /// starts the engine on it. On a device with four or more channels, such as a DDJ-FLX4's
    /// sound card, the master plays on outputs 1 and 2 and the headphones on 3 and 4.
    pub fn start_on(
        name: &str,
        config: &OutputConfig,
    ) -> Result<(Controller, AudioOutput), EngineError> {
        let device = cpal::default_host()
            .output_devices()
            .map_err(|error| EngineError::Output(error.to_string()))?
            .find(|device| device_name(device) == name)
            .ok_or_else(|| EngineError::OutputDeviceNotFound(name.to_owned()))?;
        Self::start_device(device, config)
    }

    /// Starts the engine on no sound card at all: a thread renders it in real time, at 48 kHz
    /// on four channels like a DJ controller's sound card, and throws the audio away. The decks
    /// play, loop and meter as on a real output, without a sound.
    pub fn start_silent(config: &OutputConfig) -> Result<(Controller, AudioOutput), EngineError> {
        let (controller, renderer) = offline_engine(SILENT_SAMPLE_RATE);
        let sink = SilentSink::start(
            renderer,
            config.buffer_frames.unwrap_or(SILENT_BUFFER_FRAMES),
            HEADPHONE_CHANNEL_COUNT,
        )
        .map_err(|error| EngineError::Output(error.to_string()))?;
        Ok((
            controller,
            AudioOutput {
                _sink: Sink::Silent { _sink: sink },
                device_name: SILENT_OUTPUT_NAME.to_owned(),
                channel_count: HEADPHONE_CHANNEL_COUNT,
                sample_rate: SILENT_SAMPLE_RATE,
                sample_format: SampleFormat::F32,
            },
        ))
    }

    /// Every output device, with its channel count at its default settings.
    pub fn output_devices() -> Result<Vec<OutputDevice>, EngineError> {
        let host = cpal::default_host();
        let default_name = host
            .default_output_device()
            .map(|device| device_name(&device));
        let devices = host
            .output_devices()
            .map_err(|error| EngineError::Output(error.to_string()))?;
        Ok(devices
            .filter_map(|device| {
                let config = device.default_output_config().ok()?;
                let name = device_name(&device);
                Some(OutputDevice {
                    is_default: default_name.as_deref() == Some(name.as_str()),
                    name,
                    channel_count: config.channels(),
                    sample_rate: config.sample_rate(),
                })
            })
            .collect())
    }

    fn start_device(
        device: cpal::Device,
        config: &OutputConfig,
    ) -> Result<(Controller, AudioOutput), EngineError> {
        let device_name = device_name(&device);
        let supported = device
            .default_output_config()
            .map_err(|error| EngineError::Output(error.to_string()))?;
        let buffer_size = match (config.buffer_frames, supported.buffer_size()) {
            (None, _) => BufferSize::Default,
            (Some(frames), SupportedBufferSize::Range { min, max }) => {
                BufferSize::Fixed(frames.clamp(*min, *max))
            }
            (Some(frames), SupportedBufferSize::Unknown) => BufferSize::Fixed(frames),
        };
        let stream_config = StreamConfig {
            channels: supported.channels(),
            sample_rate: supported.sample_rate(),
            buffer_size,
        };
        let sample_format = supported.sample_format();
        let (controller, renderer) = offline_engine(stream_config.sample_rate);
        let stream = match sample_format {
            SampleFormat::F32 => {
                build_stream::<f32>(&device, &stream_config, renderer, &controller)
            }
            SampleFormat::F64 => {
                build_stream::<f64>(&device, &stream_config, renderer, &controller)
            }
            SampleFormat::I16 => {
                build_stream::<i16>(&device, &stream_config, renderer, &controller)
            }
            SampleFormat::I32 => {
                build_stream::<i32>(&device, &stream_config, renderer, &controller)
            }
            SampleFormat::U16 => {
                build_stream::<u16>(&device, &stream_config, renderer, &controller)
            }
            other => Err(EngineError::Output(format!(
                "the output's sample format {other} is not supported"
            ))),
        }?;
        stream
            .play()
            .map_err(|error| EngineError::Output(error.to_string()))?;
        Ok((
            controller,
            AudioOutput {
                _sink: Sink::Device { _stream: stream },
                device_name,
                channel_count: stream_config.channels,
                sample_rate: stream_config.sample_rate,
                sample_format,
            },
        ))
    }

    /// Name of the output device.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Number of output channels. The master goes to the first two.
    pub fn channel_count(&self) -> u16 {
        self.channel_count
    }

    /// The rate the engine renders at, in hertz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Whether the device has outputs 3 and 4 for the headphones.
    pub fn has_headphone_output(&self) -> bool {
        self.channel_count >= HEADPHONE_CHANNEL_COUNT
    }
}

/// Where the engine's audio goes, for as long as it is kept.
enum Sink {
    Device { _stream: cpal::Stream },
    Silent { _sink: SilentSink },
}

/// A thread that pulls the engine at the pace of a sound card and discards what it renders.
struct SilentSink {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SilentSink {
    fn start(
        mut renderer: Renderer,
        buffer_frames: u32,
        channel_count: u16,
    ) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let channel_count = usize::from(channel_count);
        let buffer_frames = buffer_frames.max(1);
        let frames = usize::try_from(buffer_frames).unwrap_or(MAX_BLOCK_FRAMES);
        let period =
            Duration::from_secs_f64(f64::from(buffer_frames) / f64::from(SILENT_SAMPLE_RATE));
        let thread = std::thread::Builder::new()
            .name("dubplate-silent-output".to_owned())
            .spawn(move || {
                let mut buffer = vec![0.0; frames * channel_count];
                let mut deadline = Instant::now();
                while !stopping.load(Ordering::Acquire) {
                    renderer.render(&mut buffer, channel_count);
                    deadline += period;
                    match deadline.checked_duration_since(Instant::now()) {
                        Some(wait) => std::thread::sleep(wait),
                        None => deadline = Instant::now(),
                    }
                }
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for SilentSink {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().ok();
        }
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    renderer: Renderer,
    controller: &Controller,
) -> Result<cpal::Stream, EngineError>
where
    T: SizedSample + FromSample<f32>,
{
    let callback = renderer.into_device_callback::<T>(usize::from(config.channels));
    let shared = Arc::clone(controller.shared());
    device
        .build_output_stream::<T, _, _>(
            *config,
            callback,
            move |_error| {
                shared.stats.device_errors.fetch_add(1, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|error| EngineError::Output(error.to_string()))
}

impl Renderer {
    /// Turns the renderer into the callback an audio device calls for each buffer of samples
    /// in format `T`, interleaved with `channel_count` channels. The callback also records the
    /// device's latency and missed cycles from the timestamps it receives.
    pub fn into_device_callback<T>(
        self,
        channel_count: usize,
    ) -> impl FnMut(&mut [T], &OutputCallbackInfo) + Send + 'static
    where
        T: SizedSample + FromSample<f32>,
    {
        let mut callback = DeviceCallback::new(self, channel_count);
        move |output: &mut [T], info: &OutputCallbackInfo| callback.fill(output, info)
    }
}

/// Renders into a scratch buffer of `f32` and converts to the device's sample format.
struct DeviceCallback {
    renderer: Renderer,
    scratch: Vec<f32>,
    channel_count: usize,
}

impl DeviceCallback {
    fn new(renderer: Renderer, channel_count: usize) -> Self {
        let channel_count = channel_count.max(1);
        Self {
            renderer,
            scratch: vec![0.0; MAX_BLOCK_FRAMES * channel_count],
            channel_count,
        }
    }

    fn fill<T>(&mut self, output: &mut [T], info: &OutputCallbackInfo)
    where
        T: SizedSample + FromSample<f32>,
    {
        let _denormals = FlushDenormals::new();
        let timestamp = info.timestamp();
        self.renderer.observe_device_timing(
            Duration::from_nanos(u64::try_from(timestamp.callback.as_nanos()).unwrap_or(u64::MAX)),
            Duration::from_nanos(u64::try_from(timestamp.playback.as_nanos()).unwrap_or(u64::MAX)),
            output.len() / self.channel_count,
        );
        let started = self.renderer.begin_callback();
        let mut starved = false;
        for chunk in output.chunks_mut(self.scratch.len()) {
            let scratch = &mut self.scratch[..chunk.len()];
            starved |= self.renderer.render_frames(scratch, self.channel_count);
            for (sample, value) in chunk.iter_mut().zip(scratch.iter()) {
                *sample = T::from_sample(*value);
            }
        }
        self.renderer
            .end_callback(started, output.len() / self.channel_count, starved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeckId, Track};

    #[test]
    fn the_silent_output_plays_the_decks_in_real_time() {
        let (mut controller, output) = AudioOutput::start_silent(&OutputConfig::default()).unwrap();
        assert_eq!(output.device_name(), SILENT_OUTPUT_NAME);
        assert!(output.has_headphone_output());
        let samples = vec![0.1; SILENT_SAMPLE_RATE as usize * 10 * 2];
        let track = Track::from_interleaved_stereo(&samples, SILENT_SAMPLE_RATE).unwrap();
        controller.load_track(DeckId::A, track).unwrap();
        controller.play(DeckId::A).unwrap();
        let started = Instant::now();
        while controller.deck_state(DeckId::A).position_seconds < 0.2 {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the deck did not move"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(output);
        let stopped_at = controller.deck_state(DeckId::A).position_seconds;
        std::thread::sleep(Duration::from_millis(50));
        assert!((controller.deck_state(DeckId::A).position_seconds - stopped_at).abs() < 1e-9);
    }
}
