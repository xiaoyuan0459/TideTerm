/**
 * WindTerm — professional IDE-style dark theme.
 *
 * Inspired by the WindTerm terminal client: neutral charcoal backgrounds,
 * subtle panel separation, muted text hierarchy, and a calm blue accent.
 * Designed for users who prefer a clean, tool-window aesthetic.
 */

import type { Theme } from '../types';

export const windterm: Theme = {
  id: 'windterm',
  name: 'Tide Dark',
  colors: {
    bg0: '#1c1c1c',
    bg1: '#1c1c1c',
    bg2: '#333333',
    bg3: '#333333',
    bgHover: '#333333',
    bgInput: '#1c1c1c',
    bgTabActive: '#1c1c1c',

    tx1: '#bbbbbb',
    tx2: '#bbbbbb',
    tx3: '#777777',
    tx4: '#555555',
    txOnAccent: '#ffffff',

    bd1: '#444444',
    bd2: '#444444',
    bd3: '#555555',

    overlayBg: 'rgba(0, 0, 0, 0.55)',
    overlayBgStrong: 'rgba(0, 0, 0, 0.78)',
    overlayShadow: 'rgba(0, 0, 0, 0.45)',
    shadowElevated: 'rgba(0, 0, 0, 0.35)',

    ac: '#007acc',
    acHover: '#1593e5',
    acSoft: 'rgba(0, 122, 204, 0.14)',

    ai: '#c586c0',
    aiSoft: 'rgba(197, 134, 192, 0.14)',

    ok: '#5fd38d',
    warn: '#e0b840',
    err: '#f14c4c',
    info: '#007acc',

    statusRunning: '#5fd38d',
    statusConnected: '#5fd38d',
    statusDisconnected: '#777777',
  },
  xterm: {
    background: '#1c1c1c',
    foreground: '#bbbbbb',
    cursor: '#007acc',
    cursorAccent: '#1c1c1c',
    selectionBackground: 'rgba(0, 122, 204, 0.25)',
    black: '#1c1c1c',
    red: '#f14c4c',
    green: '#5fd38d',
    yellow: '#e0b840',
    blue: '#007acc',
    magenta: '#c586c0',
    cyan: '#56b6c2',
    white: '#bbbbbb',
    brightBlack: '#555555',
    brightRed: '#ff8a72',
    brightGreen: '#7ee0a0',
    brightYellow: '#f0c87a',
    brightBlue: '#7ab0e0',
    brightMagenta: '#b9a1ff',
    brightCyan: '#7ad0e0',
    brightWhite: '#ffffff',
  },
};
