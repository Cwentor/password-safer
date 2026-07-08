// ============================================
// 移动端入口（Phase 2 完整实现）
// 组装移动布局：MobileHeader + 搜索栏 + 内容区 + BottomNav
// 复用共享组件：DetailPanel/AddModal/SettingsModal/QrLoginModal/Toast
// Tab 切换：全部/收藏（列表）/添加（弹窗）/标签（标签云）/设置（弹窗）
// ============================================

import '../shared/styles/tokens.css';
import '../shared/styles/base.css';
import './styles/mobile.css';

import { setRenderers } from '../shared/lib/renderer.js';
import { loadConfig, loadPasswords, reloadConfigAndUI } from '../shared/lib/dataLoader.js';
import { showMainWindow } from '../shared/lib/api.js';
import { state } from '../shared/lib/state.js';
import { showToast, initToast } from '../shared/components/Toast.js';
import {
  initPasswordList, renderPasswordList, renderTagsCloud, updateCounts
} from '../shared/components/PasswordList.js';
import { initDetailPanel, tryCloseDetail } from '../shared/components/DetailPanel.js';
import { initAddModal, openAddModal, closeAddModal } from '../shared/components/AddModal.js';
import {
  initSettingsModal, populateSettingsForm, updateSyncUI, listenSyncEvents, openSettingsModal
} from '../shared/components/SettingsModal.js';
import { initQrLoginModal } from '../shared/components/QrLoginModal.js';
import { updateStorageInfo } from '../shared/components/StorageInfo.js';

import { getMobileHeaderHTML, initMobileHeader } from './components/MobileHeader.js';
import { getMobileSearchBarHTML } from './components/MobileSearchBar.js';
import { getBottomNavHTML, initBottomNav, setActiveTab } from './components/BottomNav.js';
import { initLongPressMenu, tryCloseLongPressMenu } from './components/LongPressMenu.js';

// 主区域 HTML（含内容容器，根据 Tab 切换）
function getMainHTML() {
  return `
  <main class="mobile-main">
    <div class="mobile-content" id="mobileContent">
      <!-- 默认显示密码列表 -->
      <div class="password-list mobile-password-list" id="passwordList">
        <div class="list-header">
          <span class="list-title" id="listTitle">全部密码</span>
          <span class="list-count" id="listCount">0 个条目</span>
        </div>
      </div>
    </div>
    <!-- 标签云容器（默认隐藏，点击标签 Tab 时显示） -->
    <div class="mobile-tags-view" id="mobileTagsView" style="display:none">
      <div class="list-header">
        <span class="list-title">标签云</span>
        <span class="list-count" id="mobileTagsCount">0 个标签</span>
      </div>
      <div class="tags-cloud" id="tagsCloud"></div>
    </div>
  </main>
  `;
}

// 详情面板 HTML（移动端从底部滑入全屏）
function getDetailHTML() {
  return `
  <div class="detail-backdrop" id="detailBackdrop"></div>
  <aside class="detail mobile-detail" id="detailPanel">
    <div class="detail-header mobile-detail-header">
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

// Toast / 确认对话框 / 弹窗 HTML（与桌面端一致，样式由 mobile.css 适配）
function getToastHTML() {
  return `
  <div class="toast" id="toast">
    <svg viewBox="0 0 24 24"><polyline points="20 6 9 17 4 12"/></svg>
    <span id="toastText">已复制到剪贴板</span>
  </div>
  `;
}

function getEditConfirmDialogHTML() {
  return `
  <div class="modal-overlay" id="editConfirmDialog">
    <div class="modal mobile-modal" style="width: 360px;">
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

function getAddModalHTML() {
  return `
  <div class="modal-overlay mobile-modal-overlay" id="addModal">
    <div class="modal mobile-modal">
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

function getSettingsModalHTML() {
  return `
  <div class="modal-overlay mobile-modal-overlay" id="settingsModal">
    <div class="modal mobile-modal" style="width:560px">
      <div class="modal-header">
        <div class="modal-title">设置</div>
        <button class="detail-close" id="settingsClose">
          <svg viewBox="0 0 24 24"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>

      <div class="settings-nav">
        <button class="settings-tab active" data-tab="sync">云同步</button>
        <button class="settings-tab" data-tab="storage">存储</button>
        <button class="settings-tab" data-tab="security">安全</button>
        <button class="settings-tab" data-tab="about">关于</button>
      </div>

      <div class="modal-body">

        <!-- Sync Tab -->
        <div class="settings-panel" id="tab-sync">
          <div class="settings-section-title">
            <svg viewBox="0 0 24 24"><path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><polyline points="21 3 21 8 16 8"/></svg>
            云同步设置
          </div>
          <p class="settings-desc">配置云端备份，将密码数据库同步到网盘。所有数据在上传前已在本地加密。</p>

          <div class="sync-providers settings-sync">
            <div class="sync-provider">
              <div class="sync-provider-icon quark">夸</div>
              <div class="sync-provider-info">
                <div class="sync-provider-name">夸克网盘</div>
                <div class="sync-provider-status" id="quarkStatus">未连接</div>
              </div>
              <div class="sync-toggle" id="quarkToggle" onclick="toggleSync('quark')">
                <div class="sync-toggle-knob"></div>
              </div>
              <button class="sync-chevron" id="quarkChevron" onclick="toggleProviderDetail('quark')" title="展开/收起">
                <svg viewBox="0 0 24 24"><polyline points="6 9 12 15 18 9"/></svg>
              </button>
            </div>

            <div class="sync-provider-detail" id="quarkDetail" style="display:none">
              <div class="detail-row">
                <span class="detail-key">备份路径</span>
                <input class="form-input mono" id="quarkRemotePath" style="padding:4px 8px;font-size:11px;flex:1;margin-left:12px;text-align:right" />
              </div>
              <div class="detail-row">
                <span class="detail-key">同步间隔</span>
                <select class="form-input mono" id="quarkInterval" style="padding:4px 8px;font-size:11px;width:auto;margin-left:12px">
                  <option value="60">1 分钟</option>
                  <option value="300">5 分钟</option>
                  <option value="600">10 分钟</option>
                  <option value="1800">30 分钟</option>
                </select>
              </div>
              <div class="detail-row">
                <span class="detail-key">上次同步</span>
                <span class="detail-val mono" id="quarkLastSync">—</span>
              </div>
              <div class="detail-row">
                <span class="detail-key">登录有效期至</span>
                <span class="detail-val mono" id="quarkExpires">—</span>
              </div>
              <div class="sync-actions">
                <button class="secondary-btn" onclick="manualSync('quark')">立即上传</button>
                <button class="secondary-btn" onclick="manualDownload('quark')">从云恢复</button>
                <button class="secondary-btn" onclick="openQuarkLogin()">重新扫码</button>
                <button class="secondary-btn" style="color:#e06b6b" onclick="logoutProvider('quark')">退出登录</button>
              </div>
            </div>
          </div>

          <div class="sync-providers settings-sync">
            <div class="sync-provider">
              <div class="sync-provider-icon baidu">百</div>
              <div class="sync-provider-info">
                <div class="sync-provider-name">百度网盘</div>
                <div class="sync-provider-status" id="baiduStatus">未连接</div>
              </div>
              <div class="sync-toggle" id="baiduToggle" onclick="toggleSync('baidu')">
                <div class="sync-toggle-knob"></div>
              </div>
              <button class="sync-chevron" id="baiduChevron" onclick="toggleProviderDetail('baidu')" title="展开/收起">
                <svg viewBox="0 0 24 24"><polyline points="6 9 12 15 18 9"/></svg>
              </button>
            </div>

            <div class="sync-provider-detail" id="baiduDetail" style="display:none">
              <div class="detail-row">
                <span class="detail-key">备份路径</span>
                <input class="form-input mono" id="baiduRemotePath" style="padding:4px 8px;font-size:11px;flex:1;margin-left:12px;text-align:right" />
              </div>
              <div class="detail-row">
                <span class="detail-key">同步间隔</span>
                <select class="form-input mono" id="baiduInterval" style="padding:4px 8px;font-size:11px;width:auto;margin-left:12px">
                  <option value="60">1 分钟</option>
                  <option value="300">5 分钟</option>
                  <option value="600">10 分钟</option>
                  <option value="1800">30 分钟</option>
                </select>
              </div>
              <div class="detail-row">
                <span class="detail-key">上次同步</span>
                <span class="detail-val mono" id="baiduLastSync">—</span>
              </div>
              <div class="detail-row">
                <span class="detail-key">登录有效期至</span>
                <span class="detail-val mono" id="baiduExpires">—</span>
              </div>
              <div class="sync-actions">
                <button class="secondary-btn" onclick="manualSync('baidu')">立即上传</button>
                <button class="secondary-btn" onclick="manualDownload('baidu')">从云恢复</button>
                <button class="secondary-btn" onclick="openBaiduLogin()">重新扫码</button>
                <button class="secondary-btn" style="color:#e06b6b" onclick="logoutProvider('baidu')">退出登录</button>
              </div>
            </div>
          </div>

        </div>

        <!-- Storage Tab -->
        <div class="settings-panel" id="tab-storage" style="display:none">
          <div class="settings-section-title">
            <svg viewBox="0 0 24 24"><ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3"/><path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"/></svg>
            本地存储
          </div>

          <div class="storage-card">
            <div class="storage-card-header">
              <span>JSON 数据库</span>
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
            <div class="about-version">版本 2.0.1</div>
            <div class="about-desc">
              本地优先的密码管理器<br>
              数据库采用 JSON 文件加密存储<br>
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

function getQrModalHTML() {
  return `
  <div class="modal-overlay mobile-modal-overlay" id="qrModal">
    <div class="modal mobile-modal" style="width:360px">
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

// ========== 初始化序列 ==========

// 显示加载骨架屏（在 app-ready 触发前占据内容区）
function showLoadingSkeleton() {
  const content = document.getElementById('mobileContent');
  if (!content) return;
  const skeleton = document.createElement('div');
  skeleton.id = 'appLoadingSkeleton';
  skeleton.style.cssText = 'display:flex;align-items:center;justify-content:center;flex-direction:column;height:60vh;color:var(--text-secondary,#888);font-size:15px;';
  skeleton.innerHTML = '<div style="margin-bottom:10px;">加载中...</div><div style="font-size:12px;opacity:.7;">正在解密数据，请稍候</div>';
  content.appendChild(skeleton);
}

function hideLoadingSkeleton() {
  const skeleton = document.getElementById('appLoadingSkeleton');
  if (skeleton) skeleton.remove();
}

// app-ready 触发后的实际初始化序列
async function doInit() {
  hideLoadingSkeleton();
  try {
    await loadConfig();
    await loadPasswords();
    updateCounts();
    updateStorageInfo();
    updateSyncUI();
    populateSettingsForm();
    listenSyncEvents();
  } catch (e) {
    showToast('初始化失败: ' + e);
  } finally {
    // 渲染完成（含失败）后显示窗口，避免启动时白闪
    await showMainWindow();
  }
}

async function init() {
  const tauriEvent = window.__TAURI__ && window.__TAURI__.event;

  // 监听 app-error：后端初始化失败时提示
  if (tauriEvent && tauriEvent.listen) {
    tauriEvent.listen('app-error', (event) => {
      hideLoadingSkeleton();
      showToast('应用初始化失败: ' + (event.payload || '未知错误'));
      showMainWindow();
    });
  }

  // 监听 app-ready：后端异步初始化完成后开始调用 invoke
  let ready = false;
  if (tauriEvent && tauriEvent.listen) {
    tauriEvent.listen('app-ready', () => {
      ready = true;
      doInit();
    });
  } else {
    // 无 Tauri 事件能力时直接初始化（兜底）
    ready = true;
    doInit();
  }

  // 超时保护：30 秒未收到 app-ready 则提示
  setTimeout(() => {
    if (!ready) {
      showToast('初始化超时，请重启应用');
      showMainWindow();
    }
  }, 30000);
}

// 安卓返回键监听：关闭最上层详情面板/弹窗
function bindBackButton() {
  document.addEventListener('keydown', (e) => {
    if (e.key !== 'Escape' && e.key !== 'Backspace') return;
    const confirmDlg = document.getElementById('editConfirmDialog');
    const addModal = document.getElementById('addModal');
    const settingsModal = document.getElementById('settingsModal');
    const qrModal = document.getElementById('qrModal');
    const detailPanel = document.getElementById('detailPanel');
    // 优先级最高：长按菜单（最顶层的浮层）
    if (tryCloseLongPressMenu()) {
      e.preventDefault();
    } else if (confirmDlg && confirmDlg.classList.contains('show')) {
      e.preventDefault();
      confirmDlg.classList.remove('show');
      state.pendingCloseAfterSave = false;
    } else if (qrModal && qrModal.classList.contains('show')) {
      e.preventDefault();
      qrModal.classList.remove('show');
    } else if (addModal && addModal.classList.contains('show')) {
      e.preventDefault();
      closeAddModal();
    } else if (settingsModal && settingsModal.classList.contains('show')) {
      e.preventDefault();
      settingsModal.classList.remove('show');
    } else if (detailPanel && detailPanel.classList.contains('show')) {
      e.preventDefault();
      tryCloseDetail();
    }
  });
}

// 挂载到 #app 元素
export function mount(appElement) {
  // 构建移动布局
  const appDiv = document.createElement('div');
  appDiv.className = 'mobile-app';
  appDiv.innerHTML = getMobileHeaderHTML() + getMobileSearchBarHTML() + getMainHTML() + getDetailHTML() + getBottomNavHTML();
  appElement.appendChild(appDiv);

  // 弹窗与 Toast 直接挂到 body（使用 fixed 定位）
  document.body.insertAdjacentHTML('beforeend', getToastHTML());
  document.body.insertAdjacentHTML('beforeend', getEditConfirmDialogHTML());
  document.body.insertAdjacentHTML('beforeend', getAddModalHTML());
  document.body.insertAdjacentHTML('beforeend', getSettingsModalHTML());
  document.body.insertAdjacentHTML('beforeend', getQrModalHTML());

  // 注册渲染函数
  setRenderers({
    renderPasswordList,
    renderTagsCloud,
    updateCounts,
    updateStorageInfo,
    updateSyncUI,
    populateSettingsForm,
    reloadConfigAndUI,
  });

  // 初始化所有组件
  initToast();
  initMobileHeader({ onSettingsClick: () => openSettingsModal() });
  initPasswordList();
  initDetailPanel();
  initAddModal();
  initQrLoginModal();
  initSettingsModal();
  initLongPressMenu();

  // 初始化底部导航：Tab 切换逻辑
  initBottomNav({
    onTabChange: (key) => {
      state.currentFilter = key;
      state.currentTag = null;
      // 切换视图：显示密码列表，隐藏标签云
      const listView = document.getElementById('mobileContent');
      const tagsView = document.getElementById('mobileTagsView');
      if (listView) listView.style.display = '';
      if (tagsView) tagsView.style.display = 'none';
      renderPasswordList();
    },
    onAddClick: () => openAddModal(),
    onSettingsClick: () => openSettingsModal(),
    onTagsClick: () => {
      // 切换到标签云视图
      const listView = document.getElementById('mobileContent');
      const tagsView = document.getElementById('mobileTagsView');
      if (listView) listView.style.display = 'none';
      if (tagsView) tagsView.style.display = '';
      // 更新标签计数
      const counter = {};
      state.passwords.forEach(p => {
        (p.tags || []).forEach(t => {
          const k = String(t).trim();
          if (!k) return;
          counter[k] = (counter[k] || 0) + 1;
        });
      });
      const countEl = document.getElementById('mobileTagsCount');
      if (countEl) countEl.textContent = `${Object.keys(counter).length} 个标签`;
      renderTagsCloud();
      // 高亮标签 Tab（取消其他高亮）
      setActiveTab('tags');
    },
  });

  // 安卓返回键 / Escape
  bindBackButton();

  // 显示加载骨架屏（app-ready 触发前占据内容区）
  showLoadingSkeleton();

  // 启动初始化序列（等待 app-ready 事件后再调用 invoke）
  init();
}
