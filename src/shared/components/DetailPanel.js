// ============================================
// DetailPanel 组件
// selectPassword / renderDetail / 内联编辑态 / 确认对话框
// ============================================

import { state } from '../lib/state.js';
import { renderers } from '../lib/renderer.js';
import { escapeHtml, escapeAttr, escapeQuotes } from '../lib/utils.js';
import { copyToClipboard } from '../lib/clipboard.js';
import { showToast } from './Toast.js';
import { updatePassword, deletePassword } from '../lib/api.js';

let detailPanelEl = null;
let detailContentEl = null;

export function initDetailPanel() {
  detailPanelEl = document.getElementById('detailPanel');
  detailContentEl = document.getElementById('detailContent');

  // 详情内容事件委托：点击可编辑字段进入编辑态、标签删除、标签新增
  if (detailContentEl) {
    detailContentEl.addEventListener('click', (e) => {
      // 阻止冒泡到 document：本委托内可能调用 enterEditMode()/renderEditTags() 重建 DOM，
      // 导致原 e.target 脱离 detailPanel，document 的外部点击监听会误判为外部点击而关闭卡片
      e.stopPropagation();
      // 标签删除
      const removeBtn = e.target.closest('.tag-remove');
      if (removeBtn && state.editMode && state.editBuffer) {
        const idx = parseInt(removeBtn.getAttribute('data-idx'), 10);
        if (!isNaN(idx)) {
          state.editBuffer.tags.splice(idx, 1);
          renderEditTags();
        }
        return;
      }
      // 标签新增
      const addBtn = e.target.closest('#tagAddBtn');
      if (addBtn && state.editMode && state.editBuffer) {
        if (document.getElementById('tagNewInput')) return;
        const input = document.createElement('input');
        input.className = 'tag-new-input';
        input.id = 'tagNewInput';
        input.placeholder = '新标签';
        addBtn.parentNode.insertBefore(input, addBtn);
        input.focus();
        const commit = () => {
          const val = (input.value || '').trim();
          if (val && !state.editBuffer.tags.includes(val)) {
            state.editBuffer.tags.push(val);
          }
          renderEditTags();
        };
        input.addEventListener('keydown', (ev) => {
          if (ev.key === 'Enter') { ev.preventDefault(); commit(); }
          else if (ev.key === 'Escape') { renderEditTags(); }
        });
        input.addEventListener('blur', commit);
        return;
      }
      // 点击可编辑字段（查看态）进入编辑态
      if (!state.editMode) {
        const fieldBtn = e.target.closest('.field-btn');
        if (fieldBtn) return; // 点击复制/眼睛按钮不进入编辑态
        const editable = e.target.closest('[data-editable]');
        if (editable) {
          enterEditMode();
        }
      }
    });
  }

  // 详情面板关闭
  const detailClose = document.getElementById('detailClose');
  if (detailClose) detailClose.addEventListener('click', tryCloseDetail);

  // 保存按钮（详情头部，编辑态显示）
  const saveBtn = document.getElementById('saveBtn');
  if (saveBtn) saveBtn.addEventListener('click', () => { saveChanges(); });

  // 编辑确认对话框
  const editConfirmYes = document.getElementById('editConfirmYes');
  if (editConfirmYes) {
    editConfirmYes.addEventListener('click', async () => {
      document.getElementById('editConfirmDialog').classList.remove('show');
      const ok = await saveChanges();
      if (ok) closeDetailCard();
      state.pendingCloseAfterSave = false;
    });
  }
  const editConfirmNo = document.getElementById('editConfirmNo');
  if (editConfirmNo) {
    editConfirmNo.addEventListener('click', () => {
      document.getElementById('editConfirmDialog').classList.remove('show');
      state.pendingCloseAfterSave = false;
      closeDetailCard();
    });
  }
  const editConfirmCancel = document.getElementById('editConfirmCancel');
  if (editConfirmCancel) {
    editConfirmCancel.addEventListener('click', () => {
      document.getElementById('editConfirmDialog').classList.remove('show');
      state.pendingCloseAfterSave = false;
    });
  }
  const editConfirmDialog = document.getElementById('editConfirmDialog');
  if (editConfirmDialog) {
    editConfirmDialog.addEventListener('click', (e) => {
      if (e.target === e.currentTarget) {
        e.currentTarget.classList.remove('show');
        state.pendingCloseAfterSave = false;
      }
    });
  }

  // 点击卡片外部关闭（编辑态有变更时走确认流程）
  document.addEventListener('click', (e) => {
    if (!detailPanelEl || !detailPanelEl.classList.contains('show')) return;
    if (detailPanelEl.contains(e.target)) return;
    const passwordList = document.getElementById('passwordList');
    if (passwordList && passwordList.contains(e.target)) return;
    const confirmDialog = document.getElementById('editConfirmDialog');
    if (confirmDialog && confirmDialog.contains(e.target)) return;
    tryCloseDetail();
  });

  // 删除密码
  const deleteBtn = document.getElementById('deleteBtn');
  if (deleteBtn) {
    deleteBtn.addEventListener('click', async () => {
      if (!state.selectedId) return;
      if (!confirm('确定要删除这个密码吗？此操作不可撤销。')) return;
      try {
        await deletePassword(state.selectedId);
        state.passwords = state.passwords.filter(p => p.id !== state.selectedId);
        closeDetailCard();
        renderers.updateCounts();
        renderers.renderTagsCloud();
        renderers.updateStorageInfo();
        showToast('密码已删除');
      } catch (e) {
        showToast('删除失败: ' + e);
      }
    });
  }

  // 编辑按钮
  const editBtn = document.getElementById('editBtn');
  if (editBtn) {
    editBtn.addEventListener('click', () => {
      if (!state.editMode) enterEditMode();
    });
  }

  // 注册 window 全局函数（供内联 HTML onclick 调用）
  window.togglePasswordVisibility = function() {
    window._passwordVisible = !window._passwordVisible;
    const editEl = document.getElementById('editPassword');
    if (editEl) {
      editEl.type = window._passwordVisible ? 'text' : 'password';
      return;
    }
    const el = document.getElementById('detailPassword');
    if (!el) return;
    if (window._passwordVisible) {
      el.textContent = window._currentPassword;
      el.classList.remove('masked');
    } else {
      el.textContent = '••••••••••';
      el.classList.add('masked');
    }
  };

  window.copyField = function(value, label) {
    copyToClipboard(value, `${label}已复制`);
  };

  window.copyInputValue = function(inputId, label) {
    const el = document.getElementById(inputId);
    if (el) copyToClipboard(el.value, `${label}已复制`);
  };
}

// ========== 选中密码并打开详情 ==========

export async function selectPassword(id) {
  state.selectedId = id;
  renderers.renderPasswordList();
  renderDetail(id);
  if (detailPanelEl) detailPanelEl.classList.add('show');
  const backdrop = document.getElementById('detailBackdrop');
  if (backdrop) backdrop.classList.add('show');
}

export function renderDetail(id) {
  const p = state.passwords.find(x => x.id === id);
  if (!p || !detailContentEl) return;

  // 进入查看态：重置编辑状态
  state.editMode = false;
  state.editBuffer = null;
  const saveBtnEl = document.getElementById('saveBtn');
  if (saveBtnEl) saveBtnEl.style.display = 'none';
  const editBtnEl = document.getElementById('editBtn');
  if (editBtnEl) editBtnEl.classList.remove('disabled');

  const strengthLabel = ['极弱', '弱', '中等', '强', '极强'][p.strength - 1] || '未知';
  const strengthClass = p.strength <= 1 ? 'weak' : p.strength <= 2 ? 'medium' : 'active';

  detailContentEl.innerHTML = `
    <div class="detail-info">
      <div class="detail-icon">${p.icon || '🔑'}</div>
      <div class="detail-name">${escapeHtml(p.name)}</div>
      <div class="detail-url">${escapeHtml(p.url || '—')}</div>
    </div>

    <div class="field-group" data-editable="username">
      <div class="field-label">用户名</div>
      <div class="field-value-wrapper">
        <div class="field-value" id="detailUsername">${escapeHtml(p.username || '—')}</div>
        <button class="field-btn" onclick="copyField('${escapeQuotes(p.username || '')}', '用户名')" title="复制">
          <svg viewBox="0 0 24 24"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
        </button>
      </div>
    </div>

    <div class="field-group" data-editable="password">
      <div class="field-label">密码</div>
      <div class="field-value-wrapper">
        <div class="field-value masked" id="detailPassword">••••••••••</div>
        <button class="field-btn" onclick="togglePasswordVisibility()" title="显示/隐藏">
          <svg id="eyeIcon" viewBox="0 0 24 24"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>
        </button>
        <button class="field-btn" onclick="copyField('${escapeQuotes(p.password)}', '密码')" title="复制">
          <svg viewBox="0 0 24 24"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
        </button>
      </div>
      <div class="strength-bar">
        ${[1,2,3,4].map(i => `<div class="strength-segment ${i <= p.strength ? strengthClass : ''}"></div>`).join('')}
      </div>
      <div class="strength-text">密码强度：${strengthLabel}</div>
    </div>

    <div class="field-group" data-editable="tags">
      <div class="field-label">标签</div>
      <div class="detail-tags">
        ${(p.tags || []).length ? (p.tags || []).map(t => `<span class="tag-chip active"><span class="tag-dot"></span>${escapeHtml(t)}</span>`).join('') : '<span style="font-size:12px;color:var(--text-dim)">无标签</span>'}
      </div>
    </div>

    <div class="field-group" data-editable="notes">
      <div class="field-label">备注</div>
      <div class="notes-box">${escapeHtml(p.notes || '无备注')}</div>
    </div>
  `;

  window._currentPassword = p.password;
  window._passwordVisible = false;
}

// ========== 内联编辑态 ==========

function enterEditMode() {
  const orig = state.passwords.find(x => x.id === state.selectedId);
  if (!orig) return;
  state.editBuffer = {
    name: orig.name,
    icon: orig.icon || '🔑',
    url: orig.url || '',
    username: orig.username || '',
    password: orig.password,
    tags: [...(orig.tags || [])],
    notes: orig.notes || ''
  };
  state.editMode = true;
  window._passwordVisible = false;

  const saveBtnEl = document.getElementById('saveBtn');
  if (saveBtnEl) saveBtnEl.style.display = 'flex';
  const editBtnEl = document.getElementById('editBtn');
  if (editBtnEl) editBtnEl.classList.add('disabled');

  detailContentEl.innerHTML = `
    <div class="detail-info">
      <div class="detail-icon">${state.editBuffer.icon}</div>
      <input class="field-edit-input" id="editName" value="${escapeAttr(state.editBuffer.name)}" placeholder="名称 / 网站" style="text-align:center;font-size:18px;font-weight:700;margin-bottom:6px;border-bottom:1px solid var(--border);padding:6px 4px;">
      <input class="field-edit-input" id="editUrl" value="${escapeAttr(state.editBuffer.url)}" placeholder="网址（可选）" style="text-align:center;font-family:var(--font-mono);font-size:12px;color:var(--text-secondary);">
    </div>

    <div class="field-group" data-editable="username">
      <div class="field-label">用户名</div>
      <div class="field-value-wrapper">
        <input class="field-edit-input" id="editUsername" value="${escapeAttr(state.editBuffer.username)}" placeholder="用户名或邮箱">
        <button class="field-btn" onclick="copyInputValue('editUsername', '用户名')" title="复制">
          <svg viewBox="0 0 24 24"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
        </button>
      </div>
    </div>

    <div class="field-group" data-editable="password">
      <div class="field-label">密码</div>
      <div class="field-value-wrapper">
        <input class="field-edit-input" id="editPassword" type="password" value="${escapeAttr(state.editBuffer.password)}">
        <button class="field-btn" onclick="togglePasswordVisibility()" title="显示/隐藏">
          <svg viewBox="0 0 24 24"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>
        </button>
        <button class="field-btn" onclick="copyInputValue('editPassword', '密码')" title="复制">
          <svg viewBox="0 0 24 24"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
        </button>
      </div>
    </div>

    <div class="field-group" data-editable="tags">
      <div class="field-label">标签</div>
      <div class="detail-tags" id="editTagsContainer">
        ${state.editBuffer.tags.map((t, i) => `<span class="tag-chip active"><span class="tag-dot"></span>${escapeHtml(t)}<button class="tag-remove" data-idx="${i}">×</button></span>`).join('')}
        <button class="tag-add-btn" id="tagAddBtn">+</button>
      </div>
    </div>

    <div class="field-group" data-editable="notes">
      <div class="field-label">备注</div>
      <textarea class="field-edit-textarea" id="editNotes" placeholder="备注（可选）">${escapeHtml(state.editBuffer.notes)}</textarea>
    </div>
  `;
}

function renderEditTags() {
  const container = document.getElementById('editTagsContainer');
  if (!container) return;
  container.innerHTML = `
    ${state.editBuffer.tags.map((t, i) => `<span class="tag-chip active"><span class="tag-dot"></span>${escapeHtml(t)}<button class="tag-remove" data-idx="${i}">×</button></span>`).join('')}
    <button class="tag-add-btn" id="tagAddBtn">+</button>
  `;
}

export function exitEditMode(refresh) {
  state.editMode = false;
  state.editBuffer = null;
  const saveBtnEl = document.getElementById('saveBtn');
  if (saveBtnEl) saveBtnEl.style.display = 'none';
  const editBtnEl = document.getElementById('editBtn');
  if (editBtnEl) editBtnEl.classList.remove('disabled');
  if (refresh && state.selectedId != null) renderDetail(state.selectedId);
}

export function closeDetailCard() {
  if (detailPanelEl) detailPanelEl.classList.remove('show');
  const backdrop = document.getElementById('detailBackdrop');
  if (backdrop) backdrop.classList.remove('show');
  state.selectedId = null;
  state.editMode = false;
  state.editBuffer = null;
  const saveBtnEl = document.getElementById('saveBtn');
  if (saveBtnEl) saveBtnEl.style.display = 'none';
  const editBtnEl = document.getElementById('editBtn');
  if (editBtnEl) editBtnEl.classList.remove('disabled');
  renderers.renderPasswordList();
}

function hasUnsavedChanges() {
  if (!state.editMode || !state.editBuffer) return false;
  const orig = state.passwords.find(x => x.id === state.selectedId);
  if (!orig) return false;
  const curName = document.getElementById('editName') ? document.getElementById('editName').value : orig.name;
  const curUrl = document.getElementById('editUrl') ? document.getElementById('editUrl').value : (orig.url || '');
  const curUsername = document.getElementById('editUsername') ? document.getElementById('editUsername').value : (orig.username || '');
  const curPassword = document.getElementById('editPassword') ? document.getElementById('editPassword').value : orig.password;
  const curNotes = document.getElementById('editNotes') ? document.getElementById('editNotes').value : (orig.notes || '');
  const curTags = state.editBuffer.tags;
  const origTags = orig.tags || [];
  return curName !== orig.name || curUrl !== (orig.url || '') || curUsername !== (orig.username || '') || curPassword !== orig.password || curNotes !== (orig.notes || '') || JSON.stringify(curTags) !== JSON.stringify(origTags);
}

export async function saveChanges() {
  if (!state.selectedId) return false;
  const orig = state.passwords.find(x => x.id === state.selectedId);
  if (!orig) return false;
  const nameEl = document.getElementById('editName');
  const urlEl = document.getElementById('editUrl');
  const usernameEl = document.getElementById('editUsername');
  const passwordEl = document.getElementById('editPassword');
  const notesEl = document.getElementById('editNotes');
  const name = nameEl ? nameEl.value.trim() : orig.name;
  const url = urlEl ? urlEl.value.trim() : '';
  const username = usernameEl ? usernameEl.value.trim() : '';
  const password = passwordEl ? passwordEl.value : '';
  const notes = notesEl ? notesEl.value.trim() : '';
  const tags = state.editBuffer ? [...state.editBuffer.tags] : [];
  const icon = orig.icon || '🔑';

  if (!name || !password) {
    showToast('请填写名称和密码');
    return false;
  }

  try {
    const result = await updatePassword(state.selectedId, { name, icon, url, username, password, tags, notes, favorite: orig.favorite });
    const idx = state.passwords.findIndex(x => x.id === state.selectedId);
    if (idx >= 0) state.passwords[idx] = result;
    renderers.renderPasswordList();
    renderers.renderTagsCloud();
    renderDetail(state.selectedId);
    showToast('已保存');
    return true;
  } catch (e) {
    showToast('保存失败: ' + e);
    return false;
  }
}

export function tryCloseDetail() {
  if (state.editMode && hasUnsavedChanges()) {
    document.getElementById('editConfirmDialog').classList.add('show');
    state.pendingCloseAfterSave = true;
  } else {
    closeDetailCard();
  }
}
