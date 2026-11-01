// ============================================
// 冲突 UI：挂起横幅 + 裁决弹窗（自挂载，不依赖静态节点）
// 横幅：sync://conflict 事件显示，sync://conflict-resolved 隐藏；
// 弹窗：解密两侧按记录 id 对齐展示，快捷三选 + 逐条改选。
// ============================================

import { showToast } from './Toast.js';
import {
  getConflictDetails, resolveConflict,
  onSyncConflict, onSyncConflictResolved,
} from '../lib/api.js';
import { escapeHtml } from '../lib/utils.js';
import { loadConfig, loadPasswords } from '../lib/dataLoader.js';
import { renderers } from '../lib/renderer.js';
import { showVaultUnlockModal } from './VaultUnlockModal.js';

const PROVIDER_NAMES = { quark: '夸克网盘', baidu: '百度网盘' };

let bannerEl = null;
let modalEl = null;
let currentProvider = null;
let details = null;

// ---------- 横幅 ----------

function ensureBannerDom() {
  if (bannerEl) return;
  bannerEl = document.createElement('div');
  bannerEl.id = 'conflictBanner';
  bannerEl.style.cssText =
    'position:fixed;left:50%;transform:translateX(-50%);bottom:24px;z-index:9000;' +
    'display:none;align-items:center;gap:10px;padding:10px 16px;border-radius:12px;' +
    'background:var(--warning-bg, rgba(255,170,0,.15));border:1px solid rgba(255,170,0,.4);' +
    'font-size:12px;box-shadow:0 4px 16px rgba(0,0,0,.2)';
  bannerEl.innerHTML = `
    <span id="conflictBannerText">检测到同步冲突</span>
    <button class="primary-btn" id="conflictBannerBtn" style="padding:4px 12px;font-size:12px">处理冲突</button>`;
  document.body.appendChild(bannerEl);
  document.getElementById('conflictBannerBtn').addEventListener('click', () => {
    const provider = currentProvider || 'quark';
    showResolveConflictModal(provider);
  });
}

function showBanner(provider) {
  ensureBannerDom();
  currentProvider = provider;
  document.getElementById('conflictBannerText').textContent =
    (PROVIDER_NAMES[provider] || '网盘') + ' 同步冲突待处理：两侧均有修改，需要你决定保留哪个版本';
  bannerEl.style.display = 'flex';
}

function hideBanner() {
  if (bannerEl) bannerEl.style.display = 'none';
}

/** 注册冲突事件监听（两个入口的 init 各调一次） */
export function initConflictWatcher() {
  onSyncConflict((e) => {
    const p = (e.payload && e.payload.provider) || 'quark';
    showBanner(p);
    showToast((e.payload && e.payload.message) || '检测到同步冲突');
  });
  onSyncConflictResolved(() => hideBanner());
  // 启动时若有历史挂起（sync_state.conflict_pending）也显示横幅
  import('../lib/api.js').then(({ getSyncState }) => {
    getSyncState().then((st) => {
      if (st && st.conflict_pending) {
        showBanner(st.conflict_pending.provider || 'quark');
      }
    }).catch(() => {});
  });
}

// ---------- 裁决弹窗 ----------

function recordCard(dto, side, defaultSide) {
  const checked = defaultSide === side ? 'checked' : '';
  // 记录名/用户名来自本地库与云端 DTO，必须转义（评审 🔴1：防存储型 XSS 直达 invoke 命令面）
  return `
    <div style="display:flex;align-items:center;gap:8px;padding:6px 8px;border-radius:8px;background:var(--bg-secondary,#f5f5f5)">
      <input type="radio" name="sel-${dto.id}" value="${side}" data-id="${dto.id}" ${checked}>
      <div style="flex:1;min-width:0">
        <div style="font-size:12px;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${escapeHtml(dto.icon || '🔑')} ${escapeHtml(dto.name)}</div>
        <div style="font-size:11px;color:var(--text-secondary)">${escapeHtml(dto.username || '—')} · 更新于 ${escapeHtml(dto.last_used || '—')}</div>
      </div>
    </div>`;
}

function ensureModalDom() {
  if (modalEl) return;
  modalEl = document.createElement('div');
  modalEl.className = 'modal-overlay';
  modalEl.id = 'resolveConflictModal';
  modalEl.innerHTML = `
    <div class="modal" style="max-width:520px;max-height:80vh;display:flex;flex-direction:column">
      <div class="modal-header">
        <span class="modal-title">解决同步冲突</span>
        <button class="icon-btn" id="rcClose">✕</button>
      </div>
      <div class="modal-body" id="rcBody" style="flex:1;overflow:auto"></div>
      <div class="modal-footer" id="rcFooter"></div>
    </div>`;
  document.body.appendChild(modalEl);
  modalEl.addEventListener('click', (e) => {
    if (e.target === modalEl) modalEl.classList.remove('show');
  });
  document.getElementById('rcClose').addEventListener('click', () => modalEl.classList.remove('show'));
}

function sectionTitle(text) {
  return `<div style="font-size:12px;font-weight:700;margin:10px 0 6px">${text}</div>`;
}

function renderDetails(provider) {
  const body = document.getElementById('rcBody');
  const footer = document.getElementById('rcFooter');
  const ol = details.only_local || [];
  const or = details.only_remote || [];
  const both = details.both || [];

  let html = '';
  html += sectionTitle(`仅本地存在（${ol.length}）——保留不动`);
  html += ol.length
    ? ol.map((d) => recordCard(d, 'local', 'local')).join('')
    : '<div style="font-size:11px;color:var(--text-secondary)">无</div>';
  html += sectionTitle(`仅云端存在（${or.length}）——保留不动`);
  html += or.length
    ? or.map((d) => recordCard(d, 'remote', 'remote')).join('')
    : '<div style="font-size:11px;color:var(--text-secondary)">无</div>';
  html += sectionTitle(`两侧都有修改（${both.length}）——选择保留哪个版本`);
  html += both.length
    ? both
        .map((pair) => {
          const newer = pair.newer || 'local';
          return `
            <div style="margin-bottom:8px">
              <div style="font-size:11px;color:var(--text-secondary);margin-bottom:2px">${escapeHtml(pair.local.name)}</div>
              <div style="display:grid;grid-template-columns:1fr 1fr;gap:6px">
                ${recordCard(pair.local, 'local', newer)}
                ${recordCard(pair.remote, 'remote', newer)}
              </div>
            </div>`;
        })
        .join('')
    : '<div style="font-size:11px;color:var(--text-secondary)">无</div>';
  body.innerHTML = html;

  footer.innerHTML = `
    <button class="secondary-btn" id="rcLocal">全部用本地</button>
    <button class="secondary-btn" id="rcRemote">全部用云端</button>
    <button class="primary-btn" id="rcMerge">智能合并（逐条取较新）</button>`;

  const collectSelections = (strategy) => {
    const sel = {};
    both.forEach((pair) => {
      const id = pair.local.id;
      if (strategy === 'custom') {
        const checked = body.querySelector(`input[name="sel-${id}"]:checked`);
        if (checked) sel[id] = checked.value;
      } else if (strategy === 'local') {
        sel[id] = 'local';
      } else if (strategy === 'remote') {
        sel[id] = 'remote';
      }
    });
    return sel;
  };

  const doResolve = async (strategy) => {
    const selections = collectSelections(strategy === 'merge' ? 'custom' : strategy);
    try {
      const r = await resolveConflict(provider, strategy, selections);
      showToast((r && r.message) || '冲突已解决');
      modalEl.classList.remove('show');
      hideBanner();
      await loadConfig();
      await loadPasswords();
      renderers.populateSettingsForm();
    } catch (e) {
      showToast('解决冲突失败: ' + e);
    }
  };
  document.getElementById('rcLocal').addEventListener('click', () => doResolve('local'));
  document.getElementById('rcRemote').addEventListener('click', () => doResolve('remote'));
  document.getElementById('rcMerge').addEventListener('click', () => doResolve('merge'));
}

export async function showResolveConflictModal(provider) {
  ensureModalDom();
  currentProvider = provider;
  document.getElementById('rcBody').innerHTML =
    '<div style="font-size:12px;color:var(--text-secondary)">正在解密两侧版本…</div>';
  document.getElementById('rcFooter').innerHTML = '';
  modalEl.classList.add('show');
  try {
    details = await getConflictDetails(provider);
    renderDetails(provider);
  } catch (e) {
    document.getElementById('rcBody').innerHTML =
      `<div style="font-size:12px;color:var(--text-secondary)">读取冲突详情失败：${escapeHtml(String(e))}</div>
      <div style="margin-top:10px"><button class="secondary-btn" id="rcAdopt">输入云端库密码接管</button></div>`;
    const adoptBtn = document.getElementById('rcAdopt');
    if (adoptBtn) {
      adoptBtn.addEventListener('click', async () => {
        modalEl.classList.remove('show');
        const ok = await showVaultUnlockModal('adopt', { provider });
        if (ok) showResolveConflictModal(provider);
      });
    }
  }
}
