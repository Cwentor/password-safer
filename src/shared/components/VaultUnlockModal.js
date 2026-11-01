// ============================================
// VaultUnlockModal：库密码三态弹窗（自挂载，不依赖静态节点）
// create：首启设置库密码（强制，忘记密码 = 数据无法找回，警示必须确认）
// unlock：v2 无本机缓存时输密码解锁（新设备路径）
// change：修改库密码（设置面板入口）
// ============================================

import { state } from '../lib/state.js';
import { showToast } from './Toast.js';
import {
  unlockVault, setVaultPassword, changeVaultPassword, migrateVaultToPassword, getVaultStatus,
} from '../lib/api.js';
import { loadConfig, loadPasswords } from '../lib/dataLoader.js';
import { renderers } from '../lib/renderer.js';

const FORGOTTEN_WARNING =
  '警告：库密码是解密密码簿的唯一凭据。忘记库密码且没有其他已解锁设备时，' +
  '本地与云端的全部数据将永久无法恢复。请务必牢记。';

let modalEl = null;

function ensureModalDom() {
  if (modalEl) return;
  modalEl = document.createElement('div');
  modalEl.className = 'modal-overlay';
  modalEl.id = 'vaultUnlockModal';
  modalEl.innerHTML = `
    <div class="modal" style="max-width:440px">
      <div class="modal-header">
        <span class="modal-title" id="vuTitle">库密码</span>
        <button class="icon-btn" id="vuClose">✕</button>
      </div>
      <div class="modal-body">
        <p id="vuDesc" style="font-size:12px;color:var(--text-secondary);margin:0 0 12px"></p>
        <div id="vuOldWrap" style="margin-bottom:10px;display:none">
          <input type="password" class="form-input" id="vuOld" placeholder="当前库密码" autocomplete="off">
        </div>
        <input type="password" class="form-input" id="vuNew" placeholder="库密码" autocomplete="off">
        <div id="vuStrength" style="font-size:11px;color:var(--text-secondary);margin-top:4px"></div>
        <div id="vuWarnWrap" style="display:none;margin-top:10px;padding:8px 10px;border-radius:8px;background:var(--warning-bg, rgba(255,170,0,.12));font-size:11px;line-height:1.6;color:var(--text-primary)">
          ${FORGOTTEN_WARNING}
          <label style="display:flex;gap:6px;align-items:center;margin-top:6px;cursor:pointer">
            <input type="checkbox" id="vuConfirm"> 我已理解并牢记库密码
          </label>
        </div>
      </div>
      <div class="modal-footer">
        <button class="secondary-btn" id="vuCancel">稍后</button>
        <button class="primary-btn" id="vuOk">确认</button>
      </div>
    </div>`;
  document.body.appendChild(modalEl);

  document.getElementById('vuNew').addEventListener('input', (e) => {
    const el = document.getElementById('vuStrength');
    const n = e.target.value.length;
    el.textContent = n === 0 ? '' : n < 8 ? '至少 8 位（当前 ' + n + ' 位）'
      : n < 12 ? '强度：一般（建议 ≥12 位或一组四个词）' : '强度：较好';
  });
  modalEl.addEventListener('click', (e) => {
    if (e.target === modalEl) cancelResolve();
  });
  document.getElementById('vuClose').addEventListener('click', cancelResolve);
  document.getElementById('vuCancel').addEventListener('click', cancelResolve);
  document.getElementById('vuOk').addEventListener('click', submit);
}

// 当前会话 Promise 的解决器（unlock/create 成功或取消时回调）
let activeResolve = null;
let mode = 'unlock';

function cancelResolve() {
  if (activeResolve) { activeResolve(false); activeResolve = null; }
  modalEl.classList.remove('show');
}

async function submit() {
  const okBtn = document.getElementById('vuOk');
  const val = document.getElementById('vuNew').value;
  try {
    okBtn.disabled = true;
    if (mode === 'create') {
      if (!document.getElementById('vuConfirm').checked) {
        showToast('请先勾选"我已理解并牢记库密码"');
        return;
      }
      await setVaultPassword(val);
    } else if (mode === 'unlock') {
      await unlockVault(val);
    } else if (mode === 'migrate') {
      if (!document.getElementById('vuConfirm').checked) {
        showToast('请先勾选"我已理解并牢记库密码"');
        return;
      }
      await migrateVaultToPassword(val);
    } else {
      const oldVal = document.getElementById('vuOld').value;
      await changeVaultPassword(oldVal, val);
    }
    // 成功：刷新数据与设置面板
    state.vaultStatus = { format: 'v2', unlocked: true };
    await loadConfig();
    await loadPasswords();
    renderers.populateSettingsForm();
    showToast(mode === 'create' ? '库密码已设置' : mode === 'unlock' ? '已解锁' : '库密码已修改');
    if (activeResolve) { activeResolve(true); activeResolve = null; }
    modalEl.classList.remove('show');
  } catch (e) {
    showToast((mode === 'unlock' ? '解锁失败: ' : '操作失败: ') + e);
  } finally {
    okBtn.disabled = false;
  }
}

/**
 * 打开库密码弹窗
 * @param {'create'|'unlock'|'change'|'migrate'} m 模式
 * @returns {Promise<boolean>} unlock/create/migrate 返回是否成功（change 恒 true）
 */
export function showVaultUnlockModal(m) {
  mode = m;
  ensureModalDom();
  const desc = {
    create: '库密码用于加密整个密码簿，也是跨设备恢复数据的唯一凭据。',
    unlock: '本机没有缓存的解锁凭据，请输入库密码解锁密码簿（新设备首次使用也在此输入）。',
    change: '修改库密码会对整个密码簿重新加密，云端将在下次同步时更新。',
    migrate: '为 master.key 模式的密码簿设置库密码：全库重新加密，之后凭库密码即可跨设备恢复。',
  }[m];
  document.getElementById('vuTitle').textContent =
    m === 'create' ? '设置库密码'
    : m === 'unlock' ? '解锁密码簿'
    : m === 'migrate' ? '升级为库密码模式'
    : '修改库密码';
  document.getElementById('vuDesc').textContent = desc;
  document.getElementById('vuOldWrap').style.display = m === 'change' ? 'block' : 'none';
  document.getElementById('vuWarnWrap').style.display = (m === 'create' || m === 'migrate') ? 'block' : 'none';
  document.getElementById('vuNew').value = '';
  const old = document.getElementById('vuOld');
  old.value = '';
  document.getElementById('vuConfirm').checked = false;
  document.getElementById('vuStrength').textContent = '';
  modalEl.classList.add('show');
  (m === 'change' ? old : document.getElementById('vuNew')).focus();

  return new Promise((resolve) => { activeResolve = resolve; });
}

/**
 * 启动分流：检查密码簿状态，需要时弹出建库/解锁框。
 * @returns {Promise<boolean>} 密码簿当前是否可用（可加载数据）
 */
export async function ensureVaultAccessible() {
  try {
    const st = await getVaultStatus();
    state.vaultStatus = st;
    if (st.format === 'none') {
      return await showVaultUnlockModal('create');
    }
    if (st.format === 'v2' && !st.unlocked) {
      const ok = await showVaultUnlockModal('unlock');
      if (!ok) showToast('密码簿保持锁定，可在设置的"安全"页解锁');
      return ok;
    }
    return true;
  } catch (e) {
    console.warn('[vault] 状态检查失败:', e);
    return true; // 检查失败按旧流程继续（v1 模式后端已自动打开）
  }
}
