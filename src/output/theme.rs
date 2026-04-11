//! Centralized color palette for terminal output.
//!
//! Every color used in text rendering is defined here so the palette
//! can be changed in one place.

use colored::CustomColor;

/// Subdued gray for metadata: index labels, permission flags, account names, log prefixes.
pub(crate) const DIM_GRAY: CustomColor = CustomColor { r: 128, g: 128, b: 128 };

/// Warm amber for fallback raw hex when structured instruction decoding fails.
pub(crate) const RAW_HEX_AMBER: CustomColor = CustomColor { r: 214, g: 154, b: 74 };

/// Green for positive balance changes and instruction names.
pub(crate) const COLOR_GREEN: (u8, u8, u8) = (152, 195, 121);

/// Red for negative balance changes and program failures.
pub(crate) const COLOR_RED: (u8, u8, u8) = (224, 108, 117);

/// Gold for instruction numbers (#1, #2, …).
pub(crate) const COLOR_GOLD: (u8, u8, u8) = (229, 192, 123);

/// Muted blue for before→after values and parsed instruction fields.
pub(crate) const COLOR_BLUE: (u8, u8, u8) = (171, 178, 191);
