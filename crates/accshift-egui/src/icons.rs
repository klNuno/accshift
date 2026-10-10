//! The SVG icons the Svelte shell draws inline, copied from `TitleBar.svelte`,
//! `ViewToggle.svelte`, `FolderCard.svelte`, `BackCard.svelte` and
//! `platformIcons.ts`. `currentColor` is spelled white so a tint gives the
//! final colour.

macro_rules! svg {
    ($view:literal, $attrs:literal, $body:literal) => {
        concat!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"",
            $view,
            "\" ",
            $attrs,
            ">",
            $body,
            "</svg>"
        )
        .as_bytes()
    };
}

pub const REFRESH: &[u8] = svg!(
    "0 0 24 24",
    "fill=\"none\" stroke=\"white\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"",
    "<path d=\"M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8\"/><path d=\"M21 3v5h-5\"/>"
);
pub const PLUS: &[u8] = svg!(
    "0 0 24 24",
    "fill=\"none\" stroke=\"white\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"",
    "<line x1=\"12\" y1=\"5\" x2=\"12\" y2=\"19\"/><line x1=\"5\" y1=\"12\" x2=\"19\" y2=\"12\"/>"
);
pub const SETTINGS: &[u8] = svg!(
    "0 0 24 24",
    "fill=\"none\" stroke=\"white\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"",
    "<path d=\"M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z\"/><circle cx=\"12\" cy=\"12\" r=\"3\"/>"
);
pub const BULK_EDIT: &[u8] = svg!(
    "0 0 24 24",
    "fill=\"none\" stroke=\"white\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"",
    "<path d=\"M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7\"/><path d=\"M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z\"/>"
);
pub const MINIMIZE: &[u8] = svg!(
    "0 0 12 12",
    "",
    "<rect x=\"1\" y=\"5.5\" width=\"10\" height=\"1\" fill=\"white\"/>"
);
pub const MAXIMIZE: &[u8] = svg!(
    "0 0 12 12",
    "",
    "<rect x=\"1.6\" y=\"1.6\" width=\"8.8\" height=\"8.8\" fill=\"none\" stroke=\"white\" stroke-width=\"1.2\"/>"
);
pub const RESTORE: &[u8] = svg!(
    "0 0 12 12",
    "",
    "<rect x=\"1.6\" y=\"3.4\" width=\"7\" height=\"7\" fill=\"none\" stroke=\"white\" stroke-width=\"1.2\"/><path d=\"M3.4 3.4V1.6h7v7H8.6\" fill=\"none\" stroke=\"white\" stroke-width=\"1.2\"/>"
);
pub const CLOSE: &[u8] = svg!(
    "0 0 12 12",
    "",
    "<path d=\"M1 1l10 10M11 1L1 11\" stroke=\"white\" stroke-width=\"1.2\"/>"
);
pub const GRID: &[u8] = svg!(
    "0 0 24 24",
    "fill=\"white\"",
    "<rect x=\"3\" y=\"3\" width=\"7\" height=\"7\" rx=\"1.5\"/><rect x=\"14\" y=\"3\" width=\"7\" height=\"7\" rx=\"1.5\"/><rect x=\"3\" y=\"14\" width=\"7\" height=\"7\" rx=\"1.5\"/><rect x=\"14\" y=\"14\" width=\"7\" height=\"7\" rx=\"1.5\"/>"
);
pub const LIST: &[u8] = svg!(
    "0 0 24 24",
    "fill=\"white\"",
    "<rect x=\"3\" y=\"4\" width=\"18\" height=\"3\" rx=\"1\"/><rect x=\"3\" y=\"10.5\" width=\"18\" height=\"3\" rx=\"1\"/><rect x=\"3\" y=\"17\" width=\"18\" height=\"3\" rx=\"1\"/>"
);
pub const FOLDER: &[u8] = svg!(
    "0 0 24 24",
    "fill=\"none\" stroke=\"white\" stroke-width=\"1.5\" stroke-linecap=\"round\" stroke-linejoin=\"round\"",
    "<path d=\"M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z\"/>"
);
pub const BACK: &[u8] = svg!(
    "0 0 24 24",
    "fill=\"none\" stroke=\"white\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"",
    "<path d=\"M19 12H5\"/><path d=\"M12 19l-7-7 7-7\"/>"
);
pub const PLAY: &[u8] = svg!("0 0 24 24", "fill=\"white\"", "<path d=\"M8 5v14l11-7z\"/>");
pub const PERSONAS: &[u8] = svg!(
    "0 0 24 24",
    "fill=\"white\"",
    "<path d=\"M16 11c1.66 0 2.99-1.34 2.99-3S17.66 5 16 5c-1.66 0-3 1.34-3 3s1.34 3 3 3zm-8 0c1.66 0 2.99-1.34 2.99-3S9.66 5 8 5C6.34 5 5 6.34 5 8s1.34 3 3 3zm0 2c-2.33 0-7 1.17-7 3.5V19h14v-2.5c0-2.33-4.67-3.5-7-3.5zm8 0c-.29 0-.62.02-.97.05 1.16.84 1.97 1.97 1.97 3.45V19h6v-2.5c0-2.33-4.67-3.5-7-3.5z\"/>"
);
pub const STEAM: &[u8] = svg!(
    "0 0 24 24",
    "fill=\"white\"",
    "<path d=\"M11.979 0C5.678 0 .511 4.86.022 11.037l6.432 2.658c.545-.371 1.203-.59 1.912-.59.063 0 .125.004.188.006l2.861-4.142V8.91c0-2.495 2.028-4.524 4.524-4.524 2.494 0 4.524 2.031 4.524 4.527s-2.03 4.525-4.524 4.525h-.105l-4.076 2.911c0 .052.004.105.004.159 0 1.875-1.515 3.396-3.39 3.396-1.635 0-3.016-1.173-3.331-2.727L.436 15.27C1.862 20.307 6.486 24 11.979 24c6.627 0 11.999-5.373 11.999-12S18.605 0 11.979 0zM7.54 18.21l-1.473-.61c.262.543.714.999 1.314 1.25 1.297.539 2.793-.076 3.332-1.375.263-.63.264-1.319.005-1.949s-.75-1.121-1.377-1.383c-.624-.26-1.29-.249-1.878-.03l1.523.63c.956.4 1.409 1.5 1.009 2.455-.397.957-1.497 1.41-2.454 1.012H7.54zm11.415-9.303c0-1.662-1.353-3.015-3.015-3.015-1.665 0-3.015 1.353-3.015 3.015 0 1.665 1.35 3.015 3.015 3.015 1.663 0 3.015-1.35 3.015-3.015zm-5.273-.005c0-1.252 1.013-2.266 2.265-2.266 1.249 0 2.266 1.014 2.266 2.266 0 1.251-1.017 2.265-2.266 2.265-1.253 0-2.265-1.014-2.265-2.265z\"/>"
);
pub const RIOT: &[u8] = svg!(
    "0 0 24 24",
    "fill=\"white\"",
    "<path d=\"M13.458.86 0 7.093l3.353 12.761 2.552-.313-.701-8.024.838-.373 1.447 8.202 4.361-.535-.775-8.857.83-.37 1.591 9.025 4.412-.542-.849-9.708.84-.374 1.74 9.87L24 17.318V3.5Zm.316 19.356.222 1.256L24 23.14v-4.18l-10.22 1.256Z\"/>"
);
pub const ROBLOX: &[u8] = svg!(
    "0 0 24 24",
    "fill=\"white\"",
    "<path d=\"M18.926 23.998 0 18.892 5.075.002 24 5.108ZM15.348 10.09l-5.282-1.453-1.414 5.273 5.282 1.453z\"/>"
);
