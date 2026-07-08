// ============================================
// QrLoginModal 组件
// 二维码扫码登录 + 轮询逻辑
// ============================================

import { state, QR_PROACTIVE_MS } from '../lib/state.js';
import { renderers } from '../lib/renderer.js';
import { showToast } from './Toast.js';
import { qrStart, qrPoll, openQuarkLogin, openBaiduLogin } from '../lib/api.js';

export function initQrLoginModal() {
  const qrModalEl = document.getElementById('qrModal');
  if (!qrModalEl) return;

  function closeQrModal() {
    state.qrPolling = false;
    state.qrStatus = 'idle';
    clearTimeout(state.qrProactiveTimer);
    qrModalEl.classList.remove('show');
  }

  const qrClose = document.getElementById('qrClose');
  if (qrClose) qrClose.addEventListener('click', closeQrModal);

  const qrRefreshBtn = document.getElementById('qrRefreshBtn');
  if (qrRefreshBtn) {
    qrRefreshBtn.addEventListener('click', () => {
      if (state.qrCurrentProvider) generateQrCode();
    });
  }

  qrModalEl.addEventListener('click', (e) => {
    if (e.target === qrModalEl) closeQrModal();
  });
}

export async function startQrLogin(provider) {
  const providerName = provider === 'quark' ? '夸克网盘' : '百度网盘';
  const qrTitle = document.getElementById('qrTitle');
  if (qrTitle) qrTitle.textContent = providerName + ' · 扫码登录';
  state.qrCurrentProvider = provider;
  const refreshBtn = document.getElementById('qrRefreshBtn');
  if (refreshBtn) refreshBtn.style.display = 'none';
  const qrModalEl = document.getElementById('qrModal');
  if (qrModalEl) qrModalEl.classList.add('show');
  await generateQrCode();
}

async function generateQrCode() {
  const imgEl = document.getElementById('qrImage');
  const stEl = document.getElementById('qrStatus');
  const refreshBtn = document.getElementById('qrRefreshBtn');
  if (imgEl) imgEl.src = '';
  if (stEl) stEl.textContent = '正在生成二维码...';
  if (refreshBtn) refreshBtn.style.display = 'none';
  try {
    const session = await qrStart(state.qrCurrentProvider);
    if (imgEl) imgEl.src = session.qr_image;
    state.qrCurrentToken = session.login_token;
    state.qrStatus = 'waiting';
    if (stEl) stEl.textContent = '请使用 ' + (state.qrCurrentProvider === 'quark' ? '夸克网盘' : '百度网盘') + ' App 扫描二维码';
    state.qrPolling = true;
    scheduleProactiveRefresh();
    pollQrLogin();
  } catch (e) {
    if (stEl) stEl.textContent = '生成二维码失败: ' + e;
    if (refreshBtn) refreshBtn.style.display = '';
  }
}

function scheduleProactiveRefresh() {
  clearTimeout(state.qrProactiveTimer);
  state.qrProactiveTimer = setTimeout(async () => {
    // 仅在仍处于等待扫码状态时主动刷新，避免打断已扫码待确认流程
    if (state.qrPolling && state.qrStatus === 'waiting') {
      await refreshQrCodeSilent();
    }
  }, QR_PROACTIVE_MS);
}

async function refreshQrCodeSilent() {
  try {
    const session = await qrStart(state.qrCurrentProvider);
    const imgEl = document.getElementById('qrImage');
    if (imgEl) imgEl.src = session.qr_image;
    state.qrCurrentToken = session.login_token;
    scheduleProactiveRefresh();
  } catch (e) {
    // 静默失败不阻断，下一次轮询若过期会走 expired 分支兜底
  }
}

async function pollQrLogin() {
  while (state.qrPolling) {
    await new Promise(r => setTimeout(r, 1500));
    if (!state.qrPolling) break;
    try {
      const res = await qrPoll(state.qrCurrentProvider, state.qrCurrentToken);
      const stEl = document.getElementById('qrStatus');
      const refreshBtn = document.getElementById('qrRefreshBtn');
      switch (res.status) {
        case 'waiting':
          state.qrStatus = 'waiting';
          if (stEl) stEl.textContent = '等待扫码...';
          if (refreshBtn) refreshBtn.style.display = 'none';
          break;
        case 'scanned':
          state.qrStatus = 'scanned';
          if (stEl) stEl.textContent = '已扫码，请在手机上确认登录';
          if (refreshBtn) refreshBtn.style.display = 'none';
          // 已扫码后取消主动刷新，避免打断确认流程
          clearTimeout(state.qrProactiveTimer);
          break;
        case 'expired':
          state.qrStatus = 'expired';
          if (stEl) stEl.textContent = '二维码已过期，正在自动刷新...';
          // 自动刷新：在循环内重新生成 token，继续轮询
          try {
            const session = await qrStart(state.qrCurrentProvider);
            const imgEl = document.getElementById('qrImage');
            if (imgEl) imgEl.src = session.qr_image;
            state.qrCurrentToken = session.login_token;
            if (stEl) stEl.textContent = '已刷新，请使用 App 扫描二维码';
            scheduleProactiveRefresh();
          } catch (e) {
            if (stEl) stEl.textContent = '二维码已过期，点击下方按钮刷新';
            if (refreshBtn) refreshBtn.style.display = '';
          }
          break;
        case 'failed':
          state.qrStatus = 'failed';
          if (stEl) stEl.textContent = '登录失败: ' + res.message;
          if (refreshBtn) refreshBtn.style.display = '';
          state.qrPolling = false;
          clearTimeout(state.qrProactiveTimer);
          break;
        case 'confirmed':
          state.qrStatus = 'confirmed';
          if (stEl) stEl.textContent = res.message;
          showToast(res.message);
          state.qrPolling = false;
          clearTimeout(state.qrProactiveTimer);
          setTimeout(() => {
            const qrModalEl = document.getElementById('qrModal');
            if (qrModalEl) qrModalEl.classList.remove('show');
            renderers.reloadConfigAndUI();
          }, 800);
          break;
      }
    } catch (e) {
      const stEl = document.getElementById('qrStatus');
      const refreshBtn = document.getElementById('qrRefreshBtn');
      if (stEl) stEl.textContent = '轮询失败: ' + e;
      if (refreshBtn) refreshBtn.style.display = '';
      state.qrPolling = false;
      clearTimeout(state.qrProactiveTimer);
    }
  }
}

// 暴露为 window 全局，供设置页 onclick 调用
window.openQuarkLogin = async function() {
  try {
    await openQuarkLogin();
    showToast('正在打开夸克登录窗口...');
  } catch (e) {
    showToast('打开夸克登录窗口失败: ' + e);
  }
};

window.openBaiduLogin = async function() {
  try {
    await openBaiduLogin();
    showToast('正在打开百度登录窗口...');
  } catch (e) {
    showToast('打开百度登录窗口失败: ' + e);
  }
};
