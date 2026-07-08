// ============================================
// 渲染函数注册表
// 组件（如 passwordService）需要触发列表/标签云/计数等重渲染，
// 但这些渲染函数定义在各自组件中（PasswordList / StorageInfo 等），
// 直接相互 import 会形成循环依赖。
// 因此 desktop/index.js 在初始化时通过 setRenderers 注册实际实现，
// 其他模块通过 renderer.renderXxx() 间接调用。
// ============================================

export const renderers = {
  renderPasswordList: () => {},
  renderTagsCloud: () => {},
  updateCounts: () => {},
  updateStorageInfo: () => {},
  updateSyncUI: () => {},
  populateSettingsForm: () => {},
  reloadConfigAndUI: () => {},
  renderDetail: () => {},
};

export function setRenderers(map) {
  Object.assign(renderers, map);
}
