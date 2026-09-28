// ============================================
// 共享界面模板：桌面 / 移动共用的模态与浮层 HTML（唯一一份）
//
// DOM ID 是本文件与共享组件（SettingsModal / AddModal / DetailPanel /
// QrLoginModal）之间的契约，两平台完全一致；mobile 参数只影响
// mobile-modal(-overlay) / mobile-detail(-header) 类名与标签页集合。
// 挂载序列、渲染注册、组件初始化、Escape/返回键关浮层栈也在此统一。
// ============================================

import { setRenderers } from '../lib/renderer.js';
import { initToast } from './Toast.js';
import { initPasswordList, renderPasswordList, renderTagsCloud, updateCounts } from './PasswordList.js';
import { initDetailPanel, tryCloseDetail } from './DetailPanel.js';
import { initAddModal, closeAddModal } from './AddModal.js';
import { initSettingsModal, initCloudSyncModal, populateSettingsForm } from './SettingsModal.js';
import { initQrLoginModal } from './QrLoginModal.js';
import { updateStorageInfo } from './StorageInfo.js';
import { state } from '../lib/state.js';

// ========== 详情面板 ==========

export function getDetailHTML(mobile) {
  return `
  <div class="detail-backdrop" id="detailBackdrop"></div>
  <aside class="detail${mobile ? ' mobile-detail' : ''}" id="detailPanel">
    <div class="detail-header${mobile ? ' mobile-detail-header' : ''}">
      <span class="detail-title">密码详情</span>
      <div class="detail-header-actions">
        <button class="detail-close" id="saveBtn" style="display:none" title="保存">
          <svg viewBox="0 0 24 24"><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"/><polyline points="17 21 17 13 7 13 7 21"/><polyline points="7 3 7 8 15 8"/></svg>
        </button>
        <button class="detail-close" id="detailClose">
          <svg viewBox="0 0 24 24"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>
    </div>
    <div class="detail-content" id="detailContent"></div>
    <div class="detail-actions" id="detailActions">
      <button class="detail-action-btn" id="editBtn">
        <svg viewBox="0 0 24 24"><path d="M12 20h9"/><path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z"/></svg>
        编辑
      </button>
      <button class="detail-action-btn danger" id="deleteBtn">
        <svg viewBox="0 0 24 24"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
        删除
      </button>
    </div>
  </aside>
  `;
}

// ========== Toast / 编辑确认 / 新建表单 ==========

export function getToastHTML() {
  return `
  <div class="toast" id="toast">
    <svg viewBox="0 0 24 24"><polyline points="20 6 9 17 4 12"/></svg>
    <span id="toastText">已复制到剪贴板</span>
  </div>
  `;
}

export function getEditConfirmDialogHTML(mobile) {
  return `
  <div class="modal-overlay" id="editConfirmDialog">
    <div class="modal${mobile ? ' mobile-modal' : ''}" style="width: 360px;">
      <div class="modal-header">
        <div class="modal-title">未保存的修改</div>
      </div>
      <div class="modal-body" style="padding: 24px;">
        <p style="margin: 0; font-size: 14px; color: var(--text-secondary);">是否保存修改？</p>
      </div>
      <div class="modal-footer" style="padding: 16px 24px; gap: 8px; justify-content: flex-end;">
        <button class="secondary-btn" id="editConfirmCancel">取消</button>
        <button class="secondary-btn" id="editConfirmNo" style="color:#ff6666">否</button>
        <button class="primary-btn" id="editConfirmYes">是</button>
      </div>
    </div>
  </div>
  `;
}

export function getAddModalHTML(mobile) {
  return `
  <div class="modal-overlay${mobile ? ' mobile-modal-overlay' : ''}" id="addModal">
    <div class="modal${mobile ? ' mobile-modal' : ''}">
      <div class="modal-header">
        <div class="modal-title">新建密码</div>
        <button class="detail-close" id="modalClose">
          <svg viewBox="0 0 24 24"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>
      <div class="modal-body">
        <div class="form-group">
          <label class="form-label">名称 / 网站</label>
          <input type="text" class="form-input" id="newName" placeholder="例如：GitHub">
        </div>
        <div class="form-group">
          <label class="form-label">网址</label>
          <input type="text" class="form-input mono" id="newUrl" placeholder="https://...">
        </div>
        <div class="form-group">
          <label class="form-label">用户名 / 邮箱</label>
          <input type="text" class="form-input mono" id="newUsername" placeholder="your@email.com">
        </div>
        <div class="form-group">
          <label class="form-label">密码</label>
          <div class="input-with-btn">
            <input type="text" class="form-input mono" id="newPassword" placeholder="输入密码...">
            <button class="secondary-btn" id="generateBtn">生成</button>
          </div>
          <div class="strength-bar" id="newStrengthBar">
            <div class="strength-segment"></div>
            <div class="strength-segment"></div>
            <div class="strength-segment"></div>
            <div class="strength-segment"></div>
          </div>
        </div>
        <div class="form-group">
          <label class="form-label">标签（用逗号分隔）</label>
          <input type="text" class="form-input mono" id="newTags" placeholder="工作, 开发, ...">
        </div>
        <div class="form-group">
          <label class="form-label">备注</label>
          <textarea class="form-input form-textarea" id="newNotes" placeholder="添加备注信息..."></textarea>
        </div>
      </div>
      <div class="modal-footer">
        <button class="secondary-btn" id="cancelBtn">取消</button>
        <button class="primary-btn" id="addSaveBtn">保存</button>
      </div>
    </div>
  </div>
  `;
}

// ========== 云同步：两个网盘区块（桌面独立弹窗与移动设置 Tab 共用） ==========

function getSyncProviderHTML(provider) {
  const isQuark = provider === 'quark';
  const first = isQuark ? '夸' : '百';
  return `
    <div class="sync-provider">
      <div class="sync-provider-icon ${provider}">${first}</div>
      <div class="sync-provider-info">
        <div class="sync-provider-name">${isQuark ? '夸克网盘' : '百度网盘'}</div>
        <div class="sync-provider-status" id="${provider}Status">未连接</div>
      </div>
      <div class="sync-toggle" id="${provider}Toggle" onclick="toggleSync('${provider}')">
        <div class="sync-toggle-knob"></div>
      </div>
      <button class="sync-chevron" id="${provider}Chevron" onclick="toggleProviderDetail('${provider}')" title="展开/收起">
        <svg viewBox="0 0 24 24"><polyline points="6 9 12 15 18 9"/></svg>
      </button>
    </div>

    <div class="sync-provider-detail" id="${provider}Detail" style="display:none">
      <div class="detail-row">
        <span class="detail-key">备份路径</span>
        <input class="form-input mono" id="${provider}RemotePath" style="padding:4px 8px;font-size:11px;flex:1;margin-left:12px;text-align:right" />
      </div>
      <div class="detail-row">
        <span class="detail-key">同步间隔</span>
        <select class="form-input mono" id="${provider}Interval" style="padding:4px 8px;font-size:11px;width:auto;margin-left:12px">
          <option value="60">1 分钟</option>
          <option value="300">5 分钟</option>
          <option value="600">10 分钟</option>
          <option value="1800">30 分钟</option>
        </select>
      </div>
      <div class="detail-row">
        <span class="detail-key">上次同步</span>
        <span class="detail-val mono" id="${provider}LastSync">—</span>
      </div>
      <div class="detail-row">
        <span class="detail-key">登录有效期至</span>
        <span class="detail-val mono" id="${provider}Expires">—</span>
      </div>
      <div class="sync-actions">
        <button class="secondary-btn" onclick="manualSync('${provider}')">立即上传</button>
        <button class="secondary-btn" onclick="manualDownload('${provider}')">从云恢复</button>
        <button class="secondary-btn" onclick="open${isQuark ? 'Quark' : 'Baidu'}Login()">重新扫码</button>
        <button class="secondary-btn" style="color:#e06b6b" onclick="logoutProvider('${provider}')">退出登录</button>
      </div>
    </div>
  `;
}

export function getSyncProvidersHTML() {
  return `
  <div class="sync-providers settings-sync">${getSyncProviderHTML('quark')}</div>
  <div class="sync-providers settings-sync">${getSyncProviderHTML('baidu')}</div>
  `;
}

// 独立云同步弹窗（桌面端专用）
export function getCloudSyncModalHTML() {
  return `
  <div class="modal-overlay" id="cloudSyncModal">
    <div class="modal" style="width:560px">
      <div class="modal-header">
        <div class="modal-title">云同步</div>
        <button class="detail-close" id="cloudSyncClose">
          <svg viewBox="0 0 24 24"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>

      <div class="modal-body">
        <div class="settings-section-title">
          <svg viewBox="0 0 24 24"><path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><polyline points="21 3 21 8 16 8"/></svg>
          云同步设置
        </div>
        <p class="settings-desc">配置云端备份，将密码数据库同步到网盘。所有数据在上传前已在本地加密。</p>
        ${getSyncProvidersHTML()}
      </div>
    </div>
  </div>
  `;
}

// ========== 设置弹窗（移动端多「云同步」Tab） ==========

export function getSettingsModalHTML(mobile) {
  const syncTab = mobile ? `
        <button class="settings-tab active" data-tab="sync">云同步</button>` : '';
  const syncPanel = mobile ? `
        <!-- Sync Tab -->
        <div class="settings-panel" id="tab-sync">
          <div class="settings-section-title">
            <svg viewBox="0 0 24 24"><path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><polyline points="21 3 21 8 16 8"/></svg>
            云同步设置
          </div>
          <p class="settings-desc">配置云端备份，将密码数据库同步到网盘。所有数据在上传前已在本地加密。</p>
          ${getSyncProvidersHTML()}
        </div>` : '';

  return `
  <div class="modal-overlay${mobile ? ' mobile-modal-overlay' : ''}" id="settingsModal">
    <div class="modal${mobile ? ' mobile-modal' : ''}" style="width:560px">
      <div class="modal-header">
        <div class="modal-title">设置</div>
        <button class="detail-close" id="settingsClose">
          <svg viewBox="0 0 24 24"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>

      <div class="settings-nav">${syncTab}
        <button class="settings-tab${mobile ? '' : ' active'}" data-tab="storage">存储</button>
        <button class="settings-tab" data-tab="security">安全</button>
        <button class="settings-tab" data-tab="about">关于</button>
      </div>

      <div class="modal-body">
${syncPanel}

        <!-- Storage Tab -->
        <div class="settings-panel" id="tab-storage"${mobile ? ' style="display:none"' : ''}>
          <div class="settings-section-title">
            <svg viewBox="0 0 24 24"><ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3"/><path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"/></svg>
            本地存储
          </div>

          <div class="storage-card">
            <div class="storage-card-header">
              <span>加密 JSON 存储</span>
              <span class="mono" style="color:var(--accent)">vault.json</span>
            </div>
            <div class="storage-card-body">
              <div class="detail-row">
                <span class="detail-key">数据库路径</span>
                <span class="detail-val mono">./data/vault.json</span>
              </div>
              <div class="detail-row">
                <span class="detail-key">当前大小</span>
                <span class="detail-val mono">2.3 MB</span>
              </div>
              <div class="detail-row">
                <span class="detail-key">密码条目</span>
                <span class="detail-val mono" id="storageCount">0 条</span>
              </div>
              <div class="detail-row">
                <span class="detail-key">加密方式</span>
                <span class="detail-val">AES-256-GCM</span>
              </div>
            </div>
            <div class="storage-card-footer">
              <button class="secondary-btn">导出数据库</button>
              <button class="secondary-btn">导入数据</button>
            </div>
          </div>
        </div>

        <!-- Security Tab -->
        <div class="settings-panel" id="tab-security" style="display:none">
          <div class="settings-section-title">
            <svg viewBox="0 0 24 24"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
            安全设置
          </div>

          <div class="setting-item">
            <div class="setting-item-info">
              <div class="setting-item-name">自动锁定</div>
              <div class="setting-item-desc">闲置一段时间后自动锁定保管箱</div>
            </div>
            <select class="form-input mono" style="width:120px;padding:8px 10px;font-size:12px">
              <option>5 分钟</option>
              <option>10 分钟</option>
              <option>30 分钟</option>
              <option>1 小时</option>
              <option>从不</option>
            </select>
          </div>

          <div class="setting-item">
            <div class="setting-item-info">
              <div class="setting-item-name">剪贴板自动清空</div>
              <div class="setting-item-desc">复制密码后自动清除剪贴板</div>
            </div>
            <select class="form-input mono" style="width:120px;padding:8px 10px;font-size:12px">
              <option>30 秒</option>
              <option>1 分钟</option>
              <option>5 分钟</option>
              <option>从不</option>
            </select>
          </div>

          <div class="setting-item">
            <div class="setting-item-info">
              <div class="setting-item-name">密码生成默认长度</div>
              <div class="setting-item-desc">新建密码时生成器的默认长度</div>
            </div>
            <select class="form-input mono" style="width:120px;padding:8px 10px;font-size:12px">
              <option>12 位</option>
              <option>16 位</option>
              <option>20 位</option>
              <option>24 位</option>
            </select>
          </div>

          <div class="setting-item">
            <div class="setting-item-info">
              <div class="setting-item-name">主密码保护</div>
              <div class="setting-item-desc">当前未设置主密码，建议开启以保护本地数据</div>
            </div>
            <button class="secondary-btn">设置主密码</button>
          </div>
        </div>

        <!-- About Tab -->
        <div class="settings-panel" id="tab-about" style="display:none">
          <div class="settings-section-title">
            <svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>
            关于 VAULT
          </div>

          <div class="about-card">
            <div class="about-logo">V</div>
            <div class="about-name">VAULT</div>
            <div class="about-version">版本 2.1.0</div>
            <div class="about-desc">
              本地优先的密码管理器<br>
              数据采用加密 JSON 文件，仅存储在本地<br>
              支持夸克网盘、百度网盘加密云同步
            </div>
            <div class="about-links">
              <span class="about-link">查看源码</span>
              <span class="about-link">反馈问题</span>
              <span class="about-link">使用手册</span>
            </div>
          </div>
        </div>

      </div>
    </div>
  </div>
  `;
}

// ========== 二维码登录弹窗 ==========

export function getQrModalHTML(mobile) {
  return `
  <div class="modal-overlay${mobile ? ' mobile-modal-overlay' : ''}" id="qrModal">
    <div class="modal${mobile ? ' mobile-modal' : ''}" style="width:360px">
      <div class="modal-header">
        <div class="modal-title" id="qrTitle">扫码登录</div>
        <button class="detail-close" id="qrClose">
          <svg viewBox="0 0 24 24"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>
      <div class="modal-body" style="text-align:center">
        <div style="display:flex;justify-content:center;margin-bottom:16px">
          <img id="qrImage" style="width:220px;height:220px;border-radius:12px;border:1px solid var(--border)" />
        </div>
        <div id="qrStatus" style="font-size:13px;color:var(--text-secondary);margin-bottom:12px">请使用网盘 App 扫描二维码</div>
        <button id="qrRefreshBtn" class="secondary-btn" style="padding:6px 16px;font-size:12px;margin-bottom:12px;display:none">
          <svg viewBox="0 0 24 24" width="14" height="14" style="vertical-align:-2px;margin-right:4px"><path d="M21 2v6h-6"/><path d="M3 12a9 9 0 0 1 15-6.7L21 8"/><path d="M3 22v-6h6"/><path d="M21 12a9 9 0 0 1-15 6.7L3 16"/></svg>
          刷新二维码
        </button>
        <div class="settings-hint" style="text-align:left">
          <svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>
          扫码登录后 Cookie 可保持约一个月以上免再次登录，失效后请重新扫码。
        </div>
      </div>
    </div>
  </div>
  `;
}

// ========== 挂载 / 注册 / 初始化 / 关浮层栈（两平台共用序列） ==========

/**
 * 把弹窗与 Toast 挂到 body（fixed 定位）
 * @param {object} opts { mobile, cloudSync } 移动端加 mobile-* 类名；桌面端额外挂独立云同步弹窗
 */
export function mountSharedOverlays({ mobile = false, cloudSync = false } = {}) {
  const parts = [
    getToastHTML(),
    getEditConfirmDialogHTML(mobile),
    getAddModalHTML(mobile),
    getSettingsModalHTML(mobile),
    getQrModalHTML(mobile),
  ];
  if (cloudSync) parts.splice(3, 0, getCloudSyncModalHTML());
  document.body.insertAdjacentHTML('beforeend', parts.join('\n'));
}

/** 注册渲染函数（标准五槽，两平台一致；供密码簿模块 / dataLoader 间接调用） */
export function registerCoreRenderers() {
  setRenderers({
    renderPasswordList,
    renderTagsCloud,
    updateCounts,
    updateStorageInfo,
    populateSettingsForm,
  });
}

/**
 * 初始化共享组件（按依赖顺序）
 * @param {object} opts { cloudSync, longPress } 桌面端初始化独立云同步弹窗，移动端初始化长按菜单
 */
export function initSharedComponents({ cloudSync = false, longPress = null } = {}) {
  initToast();
  initPasswordList();
  initDetailPanel();
  initAddModal();
  initQrLoginModal();
  if (cloudSync) initCloudSyncModal();
  initSettingsModal();
  if (longPress) longPress();
}

/**
 * Escape / 安卓返回键关浮层栈（自上而下：长按菜单 → 确认框 → 二维码 →
 * 新建 → 云同步(桌面) → 设置 → 详情；与两平台原行为一致）
 * @param {object} opts
 *   keys: 触发键（桌面 ['Escape']，移动 ['Escape','Backspace']）
 *   before: (e) => boolean，栈外优先处理（移动端长按菜单），返回 true 表示已消费
 *   cloudSync: 桌面端是否处理云同步弹窗
 *   qrModal: 是否处理二维码弹窗（桌面原不处理）
 */
export function bindOverlayEscape({ keys = ['Escape'], before = null, cloudSync = false, qrModal = false } = {}) {
  document.addEventListener('keydown', (e) => {
    if (!keys.includes(e.key)) return;
    if (before && before(e)) return;
    const confirmDlg = document.getElementById('editConfirmDialog');
    const addModal = document.getElementById('addModal');
    const syncModal = document.getElementById('cloudSyncModal');
    const settingsModal = document.getElementById('settingsModal');
    const detailPanel = document.getElementById('detailPanel');
    const qr = document.getElementById('qrModal');
    if (confirmDlg && confirmDlg.classList.contains('show')) {
      e.preventDefault();
      confirmDlg.classList.remove('show');
      state.pendingCloseAfterSave = false;
    } else if (qrModal && qr && qr.classList.contains('show')) {
      e.preventDefault();
      qr.classList.remove('show');
    } else if (addModal && addModal.classList.contains('show')) {
      e.preventDefault();
      closeAddModal();
    } else if (cloudSync && syncModal && syncModal.classList.contains('show')) {
      e.preventDefault();
      syncModal.classList.remove('show');
    } else if (settingsModal && settingsModal.classList.contains('show')) {
      e.preventDefault();
      settingsModal.classList.remove('show');
    } else if (detailPanel && detailPanel.classList.contains('show')) {
      e.preventDefault();
      tryCloseDetail();
    }
  });
}
