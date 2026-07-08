// ============================================
// 平台检测（基于 @tauri-apps/plugin-os）
// Task 5: 切换为正式平台检测 API
// ============================================

import { platform } from '@tauri-apps/plugin-os';

export function getPlatform() {
  try {
    // 返回 'windows' | 'android' | 'ios' | 'linux' | 'macos'
    return platform();
  } catch (e) {
    // fallback：非 Tauri 环境或调用失败时默认桌面端
    return 'windows';
  }
}

export function isDesktop() {
  const p = getPlatform();
  return p === 'windows' || p === 'linux' || p === 'macos';
}

export function isMobile() {
  const p = getPlatform();
  return p === 'android' || p === 'ios';
}
