use std::{
    f64::consts::TAU,
    ops::{Mul, Neg},
};

use cpal::{FromSample, Sample as _};

use crate::Block;

pub mod export;
pub mod generation;
pub mod live;

/// Return the `sample` clamped to between `threshold` and `-threshold` (inclusive).
///
/// # Panics
///
/// Panics if `threshold` is less than zero.
#[must_use]
pub fn clip<T: cpal::Sample + Ord + Neg<Output = T>>(sample: T, threshold: T) -> T {
    sample.clamp(-threshold, threshold)
}

/// Return the `sample` multiplied by `multiplier`.
#[must_use]
pub fn scale<T: cpal::Sample + Mul<Output = T>>(sample: T, multiplier: T) -> T {
    sample * multiplier
}

/// Given a `frequency` in hertz and an `amplitude`, return a function over time (in seconds) that generates a sine wave.
pub fn sine_wave<T: cpal::Sample + FromSample<f64>, const N: usize>(frequency: f64, amplitude: T) -> impl FnMut(f64) -> Block<T, N>
where
    f64: FromSample<T>,
{
    move |time| Block([T::from_sample(f64::from_sample(amplitude) * (TAU * frequency * time).sin()); N])
}

/// Given a `frequency` in hertz and an `amplitude`, return a function over time (in seconds) that generates a square wave.
pub fn square_wave<T: cpal::Sample + FromSample<f64>, const N: usize>(frequency: f64, amplitude: T) -> impl FnMut(f64) -> Block<T, N>
where
    f64: FromSample<T>,
{
    move |time| Block([T::from_sample((-1_f64).powf(2. * frequency * time) * f64::from_sample(amplitude)); N])
}

/// Given a `frequency` in hertz and an `amplitude`, return a function over time (in seconds) that generates a triangle wave.
pub fn triangle_wave<T: cpal::Sample + FromSample<f64>, const N: usize>(frequency: f64, amplitude: T) -> impl FnMut(f64) -> Block<T, N>
where
    f64: FromSample<T>,
{
    move |time| Block([T::from_sample((2. * f64::from_sample(amplitude)) * time.mul_add(frequency, -time.mul_add(frequency, 1. / 2.).floor()).abs()); N])
}

/// Given a `frequency` in hertz and an `amplitude`, return a function over time (in seconds) that generates a sawtooth wave.
pub fn sawtooth_wave<T: cpal::Sample + FromSample<f64>, const N: usize>(frequency: f64, amplitude: T) -> impl FnMut(f64) -> Block<T, N>
where
    f64: FromSample<T>,
{
    move |time| Block([T::from_sample((2. * f64::from_sample(amplitude)) * time.mul_add(frequency, -time.mul_add(frequency, 1. / 2.).floor())); N])
}

/// Return a function that generates silence.
pub fn silence<T: cpal::Sample, const N: usize>() -> impl FnMut(f64) -> Block<T, N> {
    move |_| Block([T::EQUILIBRIUM; N])
}
