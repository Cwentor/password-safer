// ============================================
// AddModal 组件
// 新建密码弹窗 + 密码生成器
// ============================================

import { state } from '../lib/state.js';
import { renderers } from '../lib/renderer.js';
import { showToast } from './Toast.js';
import { addPassword, generatePassword } from '../lib/api.js';

let addModalEl = null;

export function initAddModal() {
  addModalEl = document.getElementById('addModal');
  const addBtn = document.getElementById('addBtn');
  const modalClose = document.getElementById('modalClose');
  const cancelBtn = document.getElementById('cancelBtn');
  const generateBtn = document.getElementById('generateBtn');
  const addSaveBtn = document.getElementById('addSaveBtn');
  const newPasswordEl = document.getElementById('newPassword');

  if (addBtn) {
    addBtn.addEventListener('click', () => {
      openAddModal();
    });
  }

  if (modalClose) modalClose.addEventListener('click', closeAddModal);
  if (cancelBtn) cancelBtn.addEventListener('click', closeAddModal);
  if (addModalEl) {
    addModalEl.addEventListener('click', (e) => { if (e.target === addModalEl) closeAddModal(); });
  }

  // 生成密码 - 使用后端
  if (generateBtn) {
    generateBtn.addEventListener('click', async () => {
      try {
        const pwd = await generatePassword(16);
        const el = document.getElementById('newPassword');
        if (el) el.value = pwd;
        updateNewPasswordStrength(pwd);
      } catch (e) {
        showToast('生成密码失败: ' + e);
      }
    });
  }

  if (newPasswordEl) {
    newPasswordEl.addEventListener('input', (e) => {
      updateNewPasswordStrength(e.target.value);
    });
  }

  // 保存新密码
  if (addSaveBtn) {
    addSaveBtn.addEventListener('click', async () => {
      const name = (document.getElementById('newName').value || '').trim();
      const url = (document.getElementById('newUrl').value || '').trim();
      const username = (document.getElementById('newUsername').value || '').trim();
      const password = document.getElementById('newPassword').value || '';
      const tags = (document.getElementById('newTags').value || '').split(',').map(t => t.trim()).filter(t => t);
      const notes = (document.getElementById('newNotes').value || '').trim();

      if (!name || !password) {
        showToast('请填写名称和密码');
        return;
      }

      const iconMap = {
        'github': '🐙', 'gitlab': '🦊', 'google': '🔍', '微信': '💬', 'weixin': '💬',
        '支付宝': '💰', 'alipay': '💰', '淘宝': '🛒', 'taobao': '🛒',
        'qq': '🐧', 'bilibili': '📺', '哔哩哔哩': '📺',
        '夸克': '☁️', 'quark': '☁️', '百度': '📦', 'baidu': '📦',
        'vpn': '🔐', '邮箱': '📧', 'email': '📧', 'mail': '📧'
      };
      let icon = '🔑';
      const nameLower = name.toLowerCase();
      for (const [key, val] of Object.entries(iconMap)) {
        if (nameLower.includes(key.toLowerCase())) { icon = val; break; }
      }

      try {
        const result = await addPassword({ name, icon, url, username, password, tags, notes, favorite: false });
        state.passwords.unshift(result);
        closeAddModal();
        renderers.renderPasswordList();
        renderers.updateCounts();
        renderers.renderTagsCloud();
        renderers.updateStorageInfo();
        showToast('密码已保存');
      } catch (e) {
        showToast('保存失败: ' + e);
      }
    });
  }
}

export function openAddModal() {
  if (!addModalEl) return;
  addModalEl.classList.add('show');
  const nameEl = document.getElementById('newName');
  if (nameEl) nameEl.focus();
}

export function closeAddModal() {
  if (!addModalEl) return;
  addModalEl.classList.remove('show');
  ['newName', 'newUrl', 'newUsername', 'newPassword', 'newTags', 'newNotes'].forEach(id => {
    const el = document.getElementById(id);
    if (el) el.value = '';
  });
}

function updateNewPasswordStrength(pwd) {
  let strength = 0;
  if (pwd.length >= 8) strength++;
  if (pwd.length >= 12) strength++;
  if (/[A-Z]/.test(pwd) && /[a-z]/.test(pwd)) strength++;
  if (/[0-9]/.test(pwd) && /[^A-Za-z0-9]/.test(pwd)) strength++;
  const segments = document.querySelectorAll('#newStrengthBar .strength-segment');
  segments.forEach((seg, i) => {
    seg.classList.remove('active', 'medium', 'weak');
    if (i < strength) {
      if (strength <= 1) seg.classList.add('weak');
      else if (strength <= 2) seg.classList.add('medium');
      else seg.classList.add('active');
    }
  });
}
