// ============================================
// 冲突 UI（拍板 S1 重构）：首页公告横幅 + 逐对裁决弹窗（自挂载）
// 后台冲突已自动并集（两版都保留）且同步不阻塞；横幅提示"哪些数据存在
// 两版"，用户逐对选择保留哪个；key_mismatch 时横幅直达"输入云端库密码接管"。
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

let bannerEl = null;
let modalEl = null;
let currentProvider = null;
let details = null;

// ---------- 横幅（首页公告入口） ----------

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
    <button class="primary-btn" id="conflictBannerBtn" style="padding:4px 12px;font-size:12px">处理</button>`;
  document.body.appendChild(bannerEl);
  document.getElementById('conflictBannerBtn').addEventListener('click', () => {
    // 钥匙不匹配时横幅由 adopt 流程写定 provider
    const provider = currentProvider || 'quark';
    const isKeyMismatch = bannerEl.dataset.keyMismatch === '1';
    if (isKeyMismatch) {
      showVaultUnlockModal('adopt', { provider }).then((ok) => {
        if (ok) hideBanner();
      });
      return;
    }
    showResolveConflictModal(provider);
  });
}

function showBanner(text, provider, keyMismatch = false) {
  ensureBannerDom();
  currentProvider = provider;
  bannerEl.dataset.keyMismatch = keyMismatch ? '1' : '0';
  document.getElementById('conflictBannerText').textContent = text;
  document.getElementById('conflictBannerBtn').textContent = keyMismatch ? '输入库密码' : '处理冲突';
  bannerEl.style.display = 'flex';
}

function hideBanner() {
  if (bannerEl) bannerEl.style.display = 'none';
}

/** 依据 sync_state 显示对应横幅（事件与启动检查共用） */
function refreshBannerFromState(st, fallbackProvider) {
  if (st && st.key_mismatch) {
    showBanner('库密码已在其他设备更换，需要输入云端库密码接管', st.key_mismatch, true);
  } else if (st && (st.conflict_pairs || []).length > 0) {
    showBanner(
      `${st.conflict_pairs.length} 条记录在多设备间产生了不同版本（已自动保留两版），请在使用前处理`,
      (st.conflict_pairs[0] && st.conflict_pairs[0].provider) || fallbackProvider || 'quark',
    );
  } else {
    showBanner('检测到同步冲突', fallbackProvider || 'quark');
  }
}

/** 注册冲突事件监听（两个入口的 init 各调一次） */
export function initConflictWatcher() {
  onSyncConflict((e) => {
    const p = (e.payload && e.payload.provider) || 'quark';
    const msg = (e.payload && e.payload.message) || '检测到同步冲突';
    showToast(msg);
    import('../lib/api.js').then(({ getSyncState }) => {
      getSyncState()
        .then((st) => refreshBannerFromState(st, p))
        .catch(() => showBanner(msg, p));
    });
  });
  onSyncConflictResolved(() => hideBanner());
  // 启动时检查历史公告（用户打开应用先看到待决断的冲突）
  import('../lib/api.js').then(({ getSyncState }) => {
    getSyncState()
      .then((st) => {
        if (st && ((st.conflict_pairs || []).length > 0 || st.key_mismatch)) {
          refreshBannerFromState(st, 'quark');
        }
      })
      .catch(() => {});
  });
}

// ---------- 逐对裁决弹窗 ----------

function recordCard(dto, side, defaultSide) {
  const checked = defaultSide === side ? 'checked' : '';
  // 记录名/用户名来自本地库与云端 DTO，必须转义（防存储型 XSS 直达 invoke 命令面）
  const idFor = dto ? dto.id : 'none';
  return `
    <div style="display:flex;align-items:center;gap:8px;padding:6px 8px;border-radius:8px;background:var(--bg-secondary,#f5f5f5)">
      <input type="radio" name="pair-${idFor}" value="${side}" ${checked}>
      <div style="flex:1;min-width:0">
        <div style="font-size:12px;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${escapeHtml((dto && dto.icon) || '🔑')} ${escapeHtml((dto && dto.name) || '（已不存在）')}</div>
        <div style="font-size:11px;color:var(--text-secondary)">${escapeHtml((dto && dto.username) || '—')} · 更新于 ${escapeHtml((dto && dto.last_used) || '—')}</div>
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
        <span class="modal-title">处理冲突记录</span>
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

function renderDetails(provider) {
  const body = document.getElementById('rcBody');
  const footer = document.getElementById('rcFooter');
  const pairs = details.pairs || [];

  let html =
    '<div style="font-size:11px;color:var(--text-secondary);margin-bottom:6px">' +
    '以下记录在多设备间出现了不同版本（已自动保留两版，同步未受影响）。' +
    '请为每一对选择要保留的版本，落选版本将被删除。</div>';
  html += pairs
    .map((p, idx) => {
      const newerHint = p.remote_newer ? '云端版较新' : '本机版较新';
      // radio 组名用 local_id（唯一），选中值与 remote_id 一起提交
      return `
      <div style="margin-bottom:10px">
        <div style="font-size:11px;color:var(--text-secondary);margin-bottom:2px">#${idx + 1} · ${escapeHtml(p.name || '')}（${escapeHtml(newerHint)}）</div>
        <div style="display:grid;grid-template-columns:1fr 1fr;gap:6px">
          ${recordCard(p.local, 'local', p.remote_newer ? 'remote' : 'local')}
          ${recordCard(p.remote, 'remote', p.remote_newer ? 'remote' : 'local')}
        </div>
      </div>`;
    })
    .join('');
  body.innerHTML = html;

  footer.innerHTML = `<button class="primary-btn" id="rcApply">应用裁决</button>`;
  document.getElementById('rcApply').addEventListener('click', async () => {
    // selections 以云端副本 id（remote_id）为键：选 local → 删云端副本；选 remote → 删本机版
    const selections = {};
    pairs.forEach((p) => {
      const checked = body.querySelector(`input[name="pair-${p.local.id}"]:checked`);
      if (checked) selections[p.remote_id] = checked.value;
    });
    try {
      const r = await resolveConflict(provider, selections);
      showToast((r && r.message) || '冲突已处理');
      modalEl.classList.remove('show');
      hideBanner();
      await loadConfig();
      await loadPasswords();
      renderers.populateSettingsForm();
    } catch (e) {
      showToast('处理失败: ' + e);
    }
  });
}

export async function showResolveConflictModal(provider) {
  ensureModalDom();
  currentProvider = provider;
  document.getElementById('rcBody').innerHTML =
    '<div style="font-size:12px;color:var(--text-secondary)">正在读取冲突公告…</div>';
  document.getElementById('rcFooter').innerHTML = '';
  modalEl.classList.add('show');
  try {
    details = await getConflictDetails(provider);
    renderDetails(provider);
  } catch (e) {
    document.getElementById('rcBody').innerHTML =
      `<div style="font-size:12px;color:var(--text-secondary)">读取冲突公告失败：${escapeHtml(String(e))}</div>`;
  }
}
