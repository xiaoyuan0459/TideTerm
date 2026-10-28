/**
 * Light theme — clean, bright workspace.
 *
 * Light grey background with dark text and the TideTerm orange accent.
 * Suitable for well-lit environments.
 */

import type { Theme } from '../types';

export const light: Theme = {
  id: 'light',
  name: 'Light',
  light: true,
  colors: {
    bg0: '#f5f5f5',
    bg1: '#eaeaec',
    bg2: '#e0e0e4',
    bg3: '#d5d5da',
    bgHover: '#d0d0d6',
    bgInput: '#ffffff',
    bgTabActive: '#f5f5f5',

    tx1: '#1a1a1e',
    tx2: '#55555e',
    tx3: '#8a8a94',
    tx4: '#b0b0b8',
    txOnAccent: '#ffffff',

    bd1: '#d5d5da',
    bd2: '#c0c0c8',
    bd3: '#a5a5ae',

    overlayBg: 'rgba(0, 0, 0, 0.25)',
    overlayBgStrong: 'rgba(0, 0, 0, 0.45)',
    overlayShadow: 'rgba(0, 0, 0, 0.15)',
    shadowElevated: 'rgba(0, 0, 0, 0.12)',

    ac: '#e06820',
    acHover: '#f37d39',
    acSoft: 'rgba(224, 104, 32, 0.10)',

    ai: '#8250df',
    aiSoft: 'rgba(130, 80, 223, 0.10)',

    ok: '#2da44e',
    warn: '#bf8700',
    err: '#cf222e',
    info: '#0969da',

    statusRunning: '#e06820',
    statusConnected: '#2da44e',
    statusDisconnected: '#8a8a94',
  },
  xterm: {
    background: '#ffffff',
    foreground: '#1a1a1e',
    cursor: '#e06820',
    cursorAccent: '#ffffff',
    selectionBackground: 'rgba(224, 104, 32, 0.20)',
    black: '#1a1a1e',
    red: '#cf222e',
    green: '#2da44e',
    yellow: '#bf8700',
    blue: '#0969da',
    magenta: '#8250df',
    cyan: '#1b7c83',
    white: '#e0e0e4',
    brightBlack: '#55555e',
    brightRed: '#e03040',
    brightGreen: '#3cb55e',
    brightYellow: '#d49b00',
    brightBlue: '#218bff',
    brightMagenta: '#a371f7',
    brightCyan: '#2da4ae',
    brightWhite: '#f5f5f5',
  },
};
