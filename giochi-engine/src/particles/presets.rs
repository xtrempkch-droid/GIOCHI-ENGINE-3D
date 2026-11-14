//! Configurações reutilizáveis de efeitos comuns.

use crate::math::{Color, Vec2};

use super::system::ParticleConfig;

/// Configuração de uma explosão curta e brilhante.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::presets;
///
/// let config = presets::explosion();
/// assert!(config.lifetime > 0.0);
/// ```
pub fn explosion() -> ParticleConfig {
    ParticleConfig {
        velocity_spread: Vec2::new(220.0, 220.0),
        gravity: Vec2::new(0.0, 90.0),
        drag: 1.8,
        lifetime: 0.7,
        start_size: 7.0,
        end_size: 0.0,
        start_color: Color::rgb(255, 220, 100),
        end_color: Color::new(220, 45, 10, 0),
        ..ParticleConfig::default()
    }
}

/// Configuração de fumo que sobe e se dissipa.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::presets;
///
/// let config = presets::smoke();
/// assert!(config.end_size > config.start_size);
/// ```
pub fn smoke() -> ParticleConfig {
    ParticleConfig {
        velocity: Vec2::new(0.0, -22.0),
        velocity_spread: Vec2::new(8.0, 8.0),
        gravity: Vec2::new(0.0, -3.0),
        drag: 0.35,
        lifetime: 2.4,
        start_size: 5.0,
        end_size: 28.0,
        start_color: Color::new(150, 150, 160, 180),
        end_color: Color::new(70, 75, 85, 0),
        ..ParticleConfig::default()
    }
}

/// Configuração de um rasto pequeno e rápido.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::presets;
///
/// let config = presets::trail();
/// assert!(config.lifetime < 1.0);
/// ```
pub fn trail() -> ParticleConfig {
    ParticleConfig {
        velocity_spread: Vec2::new(8.0, 8.0),
        drag: 2.0,
        lifetime: 0.35,
        start_size: 4.0,
        end_size: 0.0,
        start_color: Color::rgb(100, 210, 255),
        end_color: Color::new(40, 100, 255, 0),
        ..ParticleConfig::default()
    }
}

/// Configuração de faíscas que caem e desaparecem.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::presets;
///
/// let config = presets::spark();
/// assert!(config.gravity.y > 0.0);
/// ```
pub fn spark() -> ParticleConfig {
    ParticleConfig {
        velocity_spread: Vec2::new(180.0, 180.0),
        gravity: Vec2::new(0.0, 180.0),
        drag: 0.2,
        lifetime: 0.55,
        start_size: 3.0,
        end_size: 0.0,
        start_color: Color::rgb(255, 245, 160),
        end_color: Color::new(255, 90, 20, 0),
        ..ParticleConfig::default()
    }
}

/// Configuração de gotas com gravidade e cor vermelha.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::presets;
///
/// let config = presets::blood();
/// assert!(config.gravity.y > 0.0);
/// ```
pub fn blood() -> ParticleConfig {
    ParticleConfig {
        velocity_spread: Vec2::new(90.0, 90.0),
        gravity: Vec2::new(0.0, 220.0),
        drag: 0.12,
        lifetime: 1.1,
        start_size: 4.0,
        end_size: 1.0,
        start_color: Color::rgb(145, 12, 24),
        end_color: Color::new(70, 4, 12, 0),
        ..ParticleConfig::default()
    }
}
