const state = {
  currentAdmin: null,
  meta: null,
  nodes: [],
  currentNodeDetail: null,
  currentSiteDetail: null,
  currentReleaseDetail: null,
  currentDnsProviderDetail: null,
  currentDnsZoneDetail: null,
  currentCertificateOrderDetail: null,
  currentOperationTemplateDetail: null,
  operationTemplates: [],
  nodeOperations: [],
  selectedNodeOperationNodeId: null,
  sites: [],
  releases: [],
  dnsProviders: [],
  dnsZones: [],
  certificateOrders: [],
};

const MANAGED_CACHE_RULES = Object.freeze([
  {
    ruleName: "images",
    extensionsField: "image_cache_extensions",
    ttlField: "image_cache_ttl",
    cacheControlField: "image_cache_control",
    enabledField: "image_cache_enabled",
    defaultExtensions: "jpg,jpeg,png,gif,webp,svg,ico,avif",
    defaultTtl: 2592000,
    defaultCacheControl: "public, max-age=2592000, immutable",
  },
  {
    ruleName: "css-js",
    extensionsField: "asset_cache_extensions",
    ttlField: "asset_cache_ttl",
    cacheControlField: "asset_cache_control",
    enabledField: "asset_cache_enabled",
    defaultExtensions: "css,js,mjs,map",
    defaultTtl: 604800,
    defaultCacheControl: "public, max-age=604800, immutable",
  },
]);

const DEFAULT_UPSTREAM_TEMPLATE = Object.freeze({
  name: "",
  endpoints: [
    {
      address: "",
      weight: 100,
      active: true,
      backup: false,
    },
  ],
});

const navigationConfig = {
  dashboard: {
    label: "概览",
    eyebrow: "Cluster Console",
    defaultSubsection: "dashboard-overview",
    subsections: {
      "dashboard-overview": {
        label: "控制台总览",
        description: "聚合节点、站点、发布、DNS 和证书状态，作为 PingoraHub 的标准运维入口。",
      },
    },
  },
  account: {
    label: "全局设置",
    eyebrow: "Global Settings",
    defaultSubsection: "account-security",
    subsections: {
      "account-security": {
        label: "全局设置",
        description: "管理平台级安全配置，当前支持更新管理员登录密码并清理其他会话。",
      },
    },
  },
  nodes: {
    label: "节点",
    eyebrow: "Node Management",
    defaultSubsection: "nodes-status",
    subsections: {
      "nodes-create": {
        label: "新增节点",
        description: "录入节点基础信息，作为后续 agent 注册、授权和站点分配的基础档案。",
      },
      "nodes-status": {
        label: "节点状态",
        description: "查看节点在线状态、配置版本和最后心跳时间。",
      },
      "nodes-detail": {
        label: "节点详情",
        description: "以整页方式查看节点版本、基础信息和承载站点，不再使用侧边抽屉。",
      },
      "nodes-monitor": {
        label: "节点监控",
        description: "从心跳延迟、版本分布和宿主标签角度快速定位异常节点，并联动 DNS 解析切换。",
      },
      "nodes-templates": {
        label: "动作模板",
        description: "维护 VIP 绑定、路由切换、服务重载等模板化动作，作为人工运维和自定义收尾能力。",
      },
      "node-template-detail": {
        label: "模板详情",
        description: "查看动作模板的参数白名单、运行用户和执行策略。",
      },
      "nodes-operations": {
        label: "节点动作",
        description: "向节点派发模板化运维任务；节点异常时的 DNS 解析切换由监控策略内置处理。",
      },
    },
  },
  sites: {
    label: "站点",
    eyebrow: "Site Management",
    defaultSubsection: "sites-status",
    subsections: {
      "sites-config": {
        label: "站点配置",
        description: "统一维护域名、协议、端口和上游配置，形成标准站点模型。",
      },
      "sites-bindings": {
        label: "节点绑定",
        description: "为站点定义主备、金丝雀和优先级，决定后续发布的目标节点集合。",
      },
      "sites-status": {
        label: "站点状态",
        description: "查看站点状态、配置版本和绑定数量，并可快速跳转到配置或绑定页面。",
      },
      "sites-detail": {
        label: "站点详情",
        description: "以整页方式查看站点基础信息、缓存策略、上游配置和绑定拓扑。",
      },
    },
  },
  releases: {
    label: "发布",
    eyebrow: "Release Center",
    defaultSubsection: "releases-records",
    subsections: {
      "releases-config": {
        label: "发布配置",
        description: "选择目标站点和发布类型，生成新的配置下发任务。",
      },
      "releases-records": {
        label: "发布记录",
        description: "跟踪版本发布结果，查看待处理、成功和失败节点的聚合状态。",
      },
      "releases-detail": {
        label: "发布详情",
        description: "查看单次发布的状态、统计和目标范围详情。",
      },
    },
  },
  dns: {
    label: "DNS",
    eyebrow: "DNS Center",
    defaultSubsection: "dns-providers",
    subsections: {
      "dns-providers": {
        label: "Provider 管理",
        description: "维护 DNS 服务商接入信息，为 Zone 同步和证书签发提供基础能力。",
      },
      "dns-zones": {
        label: "Zone 管理",
        description: "同步托管域并纳入平台，作为 DNS 变更和证书下单的目标范围。",
      },
      "dns-provider-detail": {
        label: "Provider 详情",
        description: "查看 Provider 接入配置、状态和编辑入口。",
      },
      "dns-zone-detail": {
        label: "Zone 详情",
        description: "查看 Zone 基础信息和归属 Provider。",
      },
    },
  },
  certificates: {
    label: "证书",
    eyebrow: "Certificate Center",
    defaultSubsection: "certificates-orders",
    subsections: {
      "certificates-apply": {
        label: "申请证书",
        description: "通过 DNS-01 发起 ACME 订单，推进 challenge 和签发流程。",
      },
      "certificates-orders": {
        label: "订单记录",
        description: "跟踪证书订单状态，并支持对失败订单执行重试或重置。",
      },
      "certificates-detail": {
        label: "订单详情",
        description: "查看证书订单的状态、错误和 ACME 参数详情。",
      },
    },
  },
};

const NAVIGATION_STORAGE_KEY = "pingorahub.console.activeView";
const activeView = readStoredActiveView();

document.addEventListener("DOMContentLoaded", () => {
  try {
    $("api-origin").textContent = window.location.origin;
    bindNavigation();
    bindGlobalActions();
    bindForms();
    bindNodeDetailDrawer();
    bindSiteDetailDrawer();
    bindDetailPages();
    resetSiteForm();
    bindTableActions();
    ensureBindingRow();
    activateView(activeView.section, activeView.subsection);
    refreshAll({ silent: true });
    window.setInterval(() => {
      refreshAll({ silent: true, preserveStatus: true });
    }, 15000);
  } catch (error) {
    console.error("failed to initialize PingoraHub console", error);
    safeToast(error.message || "控制台初始化失败，请刷新后重试", "error");
  } finally {
    document.body.classList.remove("app-booting");
  }
});

function $(id) {
  return document.getElementById(id);
}

function bindNavigation() {
  for (const item of document.querySelectorAll(".nav-parent")) {
    item.addEventListener("click", () => {
      const sectionName = item.dataset.menuGroup;
      const subsectionName =
        item.dataset.defaultSubsection ||
        navigationConfig[sectionName]?.defaultSubsection;
      activateView(sectionName, subsectionName);
    });
  }

  for (const item of document.querySelectorAll(".nav-child")) {
    item.addEventListener("click", () =>
      activateView(item.dataset.section, item.dataset.subsection),
    );
  }
}

function activateView(sectionName, subsectionName) {
  const sectionConfig = navigationConfig[sectionName];
  if (!sectionConfig) {
    return;
  }
  const resolvedSubsection =
    subsectionName || sectionConfig.defaultSubsection || Object.keys(sectionConfig.subsections)[0];
  const subsectionConfig = sectionConfig.subsections[resolvedSubsection];
  if (!subsectionConfig) {
    return;
  }

  activeView.section = sectionName;
  activeView.subsection = resolvedSubsection;

  for (const group of document.querySelectorAll(".nav-group")) {
    group.classList.toggle("active", group.dataset.group === sectionName);
  }
  for (const parent of document.querySelectorAll(".nav-parent")) {
    parent.classList.toggle("active", parent.dataset.menuGroup === sectionName);
  }
  for (const submenu of document.querySelectorAll(".nav-submenu")) {
    submenu.classList.toggle("active", submenu.dataset.submenu === sectionName);
  }
  for (const child of document.querySelectorAll(".nav-child")) {
    child.classList.toggle(
      "active",
      child.dataset.section === sectionName &&
        child.dataset.subsection === resolvedSubsection,
    );
  }
  for (const section of document.querySelectorAll(".section-panel")) {
    section.classList.toggle("active", section.id === `section-${sectionName}`);
  }
  for (const subsection of document.querySelectorAll(".subsection-panel")) {
    subsection.classList.toggle(
      "active",
      subsection.id === `subsection-${resolvedSubsection}`,
    );
  }

  $("page-group-title").textContent = sectionConfig.eyebrow;
  $("page-title").textContent = subsectionConfig.label;
  $("page-description").textContent = subsectionConfig.description;
  persistActiveView(sectionName, resolvedSubsection);
}

function readStoredActiveView() {
  const fallback = {
    section: "dashboard",
    subsection: "dashboard-overview",
  };
  try {
    const raw = window.localStorage?.getItem(NAVIGATION_STORAGE_KEY);
    if (!raw) {
      return fallback;
    }
    const parsed = JSON.parse(raw);
    const sectionConfig = navigationConfig[parsed?.section];
    if (!sectionConfig) {
      return fallback;
    }
    const subsection =
      parsed?.subsection ||
      sectionConfig.defaultSubsection ||
      Object.keys(sectionConfig.subsections)[0];
    if (!sectionConfig.subsections[subsection] || isDetailSubsection(subsection)) {
      return fallback;
    }
    return {
      section: parsed.section,
      subsection,
    };
  } catch {
    return fallback;
  }
}

function persistActiveView(sectionName, subsectionName) {
  if (isDetailSubsection(subsectionName)) {
    return;
  }
  try {
    window.localStorage?.setItem(
      NAVIGATION_STORAGE_KEY,
      JSON.stringify({
        section: sectionName,
        subsection: subsectionName,
      }),
    );
  } catch {
    // Storage can be unavailable in private browsing or strict embedded contexts.
  }
}

function isDetailSubsection(subsectionName) {
  return String(subsectionName || "").includes("detail");
}

function bindGlobalActions() {
  if ($("refresh-all")) {
    $("refresh-all").addEventListener("click", () => refreshAll());
  }
  $("logout-button").addEventListener("click", handleLogout);
  document.addEventListener("click", handleDocumentClick);
}

function bindNodeDetailDrawer() {
  $("node-detail-back").addEventListener("click", closeNodeDetailDrawer);
  $("node-detail-open-operations").addEventListener("click", () => {
    const nodeId = state.currentNodeDetail?.node_id;
    if (!nodeId) {
      return;
    }
    closeNodeDetailDrawer();
    state.selectedNodeOperationNodeId = nodeId;
    activateView("nodes", "nodes-operations");
    syncNodeOperationSelection();
    refreshNodeOperations({ silent: true });
  });
}

function bindSiteDetailDrawer() {
  $("site-detail-back").addEventListener("click", closeSiteDetailDrawer);
  $("site-detail-open-edit").addEventListener("click", async () => {
    const siteId = state.currentSiteDetail?.site_id;
    if (!siteId) {
      return;
    }
    closeSiteDetailDrawer();
    await loadSiteIntoForm(siteId, { subsection: "sites-config" });
  });
}

function bindDetailPages() {
  $("release-detail-back").addEventListener("click", () =>
    activateView("releases", "releases-records"),
  );
  $("dns-provider-detail-back").addEventListener("click", () =>
    activateView("dns", "dns-providers"),
  );
  $("dns-zone-detail-back").addEventListener("click", () =>
    activateView("dns", "dns-zones"),
  );
  $("certificate-detail-back").addEventListener("click", () =>
    activateView("certificates", "certificates-orders"),
  );
  $("operation-template-detail-back").addEventListener("click", () =>
    activateView("nodes", "nodes-templates"),
  );
  $("dns-provider-detail-open-edit").addEventListener("click", () => {
    if (state.currentDnsProviderDetail) {
      hydrateDnsProviderForm(state.currentDnsProviderDetail);
      activateView("dns", "dns-providers");
    }
  });
  $("dns-provider-detail-delete").addEventListener("click", async () => {
    const detail = state.currentDnsProviderDetail;
    if (!detail) {
      return;
    }
    if (!window.confirm(`确认删除 Provider ${detail.name} 吗？`)) {
      return;
    }
    await submitAction(
      `/api/admin/dns/providers/${detail.provider_id}`,
      "DELETE",
      null,
      "dns-provider-form-result",
      {
        successMessage: `Provider ${detail.name} 已删除`,
        refreshAfter: true,
      },
    );
    activateView("dns", "dns-providers");
  });
  $("dns-zone-detail-toggle").addEventListener("click", async () => {
    const detail = state.currentDnsZoneDetail;
    if (!detail) {
      return;
    }
    const enable = detail.status === "disabled";
    const path = enable ? "enable" : "disable";
    await submitAction(
      `/api/admin/dns/zones/${detail.zone_id}/${path}`,
      "POST",
      null,
      "dns-zone-sync-result",
      {
        successMessage: enable ? "Zone 已启用" : "Zone 已禁用",
        refreshAfter: true,
      },
    );
    activateView("dns", "dns-zones");
  });
  $("dns-zone-detail-delete").addEventListener("click", async () => {
    const detail = state.currentDnsZoneDetail;
    if (!detail) {
      return;
    }
    if (!window.confirm(`确认删除 Zone ${detail.zone_name} 吗？`)) {
      return;
    }
    await submitAction(
      `/api/admin/dns/zones/${detail.zone_id}`,
      "DELETE",
      null,
      "dns-zone-sync-result",
      {
        successMessage: `Zone ${detail.zone_name} 已删除`,
        refreshAfter: true,
      },
    );
    activateView("dns", "dns-zones");
  });
  $("certificate-detail-retry").addEventListener("click", async () => {
    const detail = state.currentCertificateOrderDetail;
    if (!detail) {
      return;
    }
    await submitAction(
      `/api/admin/certificates/orders/${detail.order_id}/retry`,
      "POST",
      null,
      "certificate-order-result",
      {
        successMessage: "证书订单已重试",
        refreshAfter: true,
      },
    );
    activateView("certificates", "certificates-orders");
  });
  $("certificate-detail-reset").addEventListener("click", async () => {
    const detail = state.currentCertificateOrderDetail;
    if (!detail) {
      return;
    }
    await submitAction(
      `/api/admin/certificates/orders/${detail.order_id}/reset`,
      "POST",
      null,
      "certificate-order-result",
      {
        successMessage: "证书订单已重置",
        refreshAfter: true,
      },
    );
    activateView("certificates", "certificates-orders");
  });
  $("operation-template-detail-open-edit").addEventListener("click", () => {
    if (state.currentOperationTemplateDetail) {
      hydrateOperationTemplateForm(state.currentOperationTemplateDetail);
      activateView("nodes", "nodes-templates");
    }
  });
  $("operation-template-detail-delete").addEventListener("click", async () => {
    const detail = state.currentOperationTemplateDetail;
    if (!detail) {
      return;
    }
    if (!window.confirm(`确认删除动作模板 ${detail.name} 吗？`)) {
      return;
    }
    await submitAction(
      `/api/admin/operations/templates/${detail.template_id}`,
      "DELETE",
      null,
      "operation-template-form-result",
      {
        successMessage: `动作模板 ${detail.name} 已删除`,
        refreshAfter: true,
      },
    );
    activateView("nodes", "nodes-templates");
  });
}

function handleDocumentClick(event) {
  for (const menu of document.querySelectorAll(".action-menu[open]")) {
    if (!menu.contains(event.target)) {
      menu.open = false;
    }
  }
}

function bindForms() {
  $("change-password-form").addEventListener("submit", handleChangePasswordSubmit);
  $("node-form").addEventListener("submit", handleNodeCreate);
  $("site-form").addEventListener("submit", handleSiteSubmit);
  $("site-form-reset").addEventListener("click", resetSiteForm);
  $("add-upstream-group").addEventListener("click", () => {
    const composerName = $("upstream-new-name").value.trim();
    addUpstreamGroup({
      ...DEFAULT_UPSTREAM_TEMPLATE,
      name: composerName || DEFAULT_UPSTREAM_TEMPLATE.name,
      endpoints: DEFAULT_UPSTREAM_TEMPLATE.endpoints.map((endpoint) => ({ ...endpoint })),
    });
    syncSiteConfigPreview();
  });
  $("upstream-groups").addEventListener("click", handleUpstreamGroupClick);
  $("upstream-groups").addEventListener("input", syncSiteConfigPreview);
  $("upstream-groups").addEventListener("change", syncSiteConfigPreview);
  for (const rule of MANAGED_CACHE_RULES) {
    $("site-form")[rule.enabledField].addEventListener("change", syncSiteConfigPreview);
    $("site-form")[rule.extensionsField].addEventListener("change", syncSiteConfigPreview);
    $("site-form")[rule.ttlField].addEventListener("change", syncSiteConfigPreview);
    $("site-form")[rule.cacheControlField].addEventListener("change", syncSiteConfigPreview);
  }
  $("binding-form").addEventListener("submit", handleBindingsSubmit);
  $("add-binding-row").addEventListener("click", () => addBindingRow());
  $("binding-rows").addEventListener("click", (event) => {
    const button = event.target.closest("[data-remove-binding]");
    if (!button) {
      return;
    }
    button.closest(".binding-row")?.remove();
    ensureBindingRow();
  });
  $("release-form").addEventListener("submit", handleReleaseSubmit);
  $("dns-provider-form").addEventListener("submit", handleDnsProviderSubmit);
  $("dns-zone-sync-form").addEventListener("submit", handleDnsZoneSyncSubmit);
  $("certificate-order-form").addEventListener("submit", handleCertificateOrderSubmit);
  $("operation-template-form").addEventListener("submit", handleOperationTemplateSubmit);
  $("node-operation-form").addEventListener("submit", handleNodeOperationSubmit);
  $("node-operation-node-id").addEventListener("change", handleNodeOperationNodeChange);
}

function bindTableActions() {
  $("node-rows").addEventListener("click", async (event) => {
    const openDetailButton = event.target.closest("[data-open-node-detail]");
    if (openDetailButton) {
      closeActionMenu(openDetailButton);
      await openNodeDetailDrawer(openDetailButton.dataset.openNodeDetail);
      return;
    }

    const openOperationsButton = event.target.closest("[data-open-node-operations]");
    if (openOperationsButton) {
      closeActionMenu(openOperationsButton);
      const nodeId = openOperationsButton.dataset.openNodeOperations;
      state.selectedNodeOperationNodeId = nodeId || null;
      activateView("nodes", "nodes-operations");
      syncNodeOperationSelection();
      await refreshNodeOperations({ silent: true });
      toast("节点动作视图已切换", "success");
      return;
    }

    const deleteButton = event.target.closest("[data-delete-node]");
    if (!deleteButton) {
      return;
    }
    closeActionMenu(deleteButton);

    const nodeId = deleteButton.dataset.deleteNode;
    const nodeCode = deleteButton.dataset.nodeCode || nodeId;
    const confirmed = window.confirm(
      `确认移除节点 ${nodeCode} 吗？未绑定站点的历史心跳、凭据和发布状态会一起清理。`,
    );
    if (!confirmed) {
      return;
    }

    await submitAction(`/api/admin/nodes/${nodeId}`, "DELETE", null, "node-form-result", {
      successMessage: `节点 ${nodeCode} 已移除`,
      refreshAfter: true,
    });
  });

  $("site-rows").addEventListener("click", async (event) => {
    const detailButton = event.target.closest("[data-open-site-detail]");
    if (detailButton) {
      closeActionMenu(detailButton);
      await openSiteDetailDrawer(detailButton.dataset.openSiteDetail);
      return;
    }

    const editButton = event.target.closest("[data-edit-site]");
    if (editButton) {
      closeActionMenu(editButton);
      await loadSiteIntoForm(editButton.dataset.editSite, {
        subsection: "sites-config",
      });
      return;
    }

    const configButton = event.target.closest("[data-edit-site-config]");
    if (configButton) {
      closeActionMenu(configButton);
      await loadSiteIntoForm(configButton.dataset.editSiteConfig, {
        subsection: "sites-config",
      });
      return;
    }

    const bindingButton = event.target.closest("[data-edit-site-bindings]");
    if (bindingButton) {
      closeActionMenu(bindingButton);
      await loadSiteIntoForm(bindingButton.dataset.editSiteBindings, {
        subsection: "sites-bindings",
        toastMessage: false,
      });
      toast("站点绑定已加载", "success");
      return;
    }

    const enableButton = event.target.closest("[data-enable-site]");
    if (enableButton) {
      closeActionMenu(enableButton);
      await submitAction(
        `/api/admin/sites/${enableButton.dataset.enableSite}/enable`,
        "POST",
        null,
        "site-status-result",
        {
          successMessage: "站点已启用",
          refreshAfter: true,
        },
      );
      return;
    }

    const disableButton = event.target.closest("[data-disable-site]");
    if (disableButton) {
      closeActionMenu(disableButton);
      await submitAction(
        `/api/admin/sites/${disableButton.dataset.disableSite}/disable`,
        "POST",
        null,
        "site-status-result",
        {
          successMessage: "站点已禁用",
          refreshAfter: true,
        },
      );
      return;
    }

    const deleteButton = event.target.closest("[data-delete-site]");
    if (deleteButton) {
      closeActionMenu(deleteButton);
      const siteId = deleteButton.dataset.deleteSite;
      const siteName = deleteButton.dataset.siteName || siteId;
      if (!window.confirm(`确认删除站点 ${siteName} 吗？已绑定节点会收到清理发布。`)) {
        return;
      }
      await submitAction(
        `/api/admin/sites/${siteId}`,
        "DELETE",
        null,
        "site-status-result",
        {
          successMessage: `站点 ${siteName} 已删除`,
          refreshAfter: true,
        },
      );
      closeSiteDetailDrawer();
      return;
    }

    const renewButton = event.target.closest("[data-renew-site-certificate]");
    if (renewButton) {
      closeActionMenu(renewButton);
      await submitAction(
        `/api/admin/sites/${renewButton.dataset.renewSiteCertificate}/renew-certificate`,
        "POST",
        null,
        "site-status-result",
        {
          successMessage: "续签订单已创建",
          refreshAfter: true,
        },
      );
      activateView("certificates", "certificates-orders");
      return;
    }

    const switchButton = event.target.closest("[data-switch-site-primary]");
    if (switchButton) {
      closeActionMenu(switchButton);
      await submitAction(
        `/api/admin/sites/${switchButton.dataset.switchSitePrimary}/switch-primary`,
        "POST",
        {},
        "site-status-result",
        {
          successMessage: "主节点切换发布已创建",
          refreshAfter: true,
        },
      );
      activateView("releases", "releases-records");
    }
  });

  $("dashboard-site-rows").addEventListener("click", async (event) => {
    const detailButton = event.target.closest("[data-open-site-detail]");
    if (detailButton) {
      closeActionMenu(detailButton);
      await openSiteDetailDrawer(detailButton.dataset.openSiteDetail);
      return;
    }

    const editButton = event.target.closest("[data-edit-site-config]");
    if (editButton) {
      closeActionMenu(editButton);
      await loadSiteIntoForm(editButton.dataset.editSiteConfig, {
        subsection: "sites-config",
      });
      return;
    }

    const enableButton = event.target.closest("[data-enable-site]");
    if (enableButton) {
      closeActionMenu(enableButton);
      await submitAction(
        `/api/admin/sites/${enableButton.dataset.enableSite}/enable`,
        "POST",
        null,
        "site-status-result",
        {
          successMessage: "站点已启用",
          refreshAfter: true,
        },
      );
      return;
    }

    const disableButton = event.target.closest("[data-disable-site]");
    if (disableButton) {
      closeActionMenu(disableButton);
      await submitAction(
        `/api/admin/sites/${disableButton.dataset.disableSite}/disable`,
        "POST",
        null,
        "site-status-result",
        {
          successMessage: "站点已禁用",
          refreshAfter: true,
        },
      );
      return;
    }

    const deleteButton = event.target.closest("[data-delete-site]");
    if (!deleteButton) {
      return;
    }

    closeActionMenu(deleteButton);
    const siteId = deleteButton.dataset.deleteSite;
    const siteName = deleteButton.dataset.siteName || siteId;
    if (!window.confirm(`确认删除站点 ${siteName} 吗？已绑定节点会收到清理发布。`)) {
      return;
    }
    await submitAction(`/api/admin/sites/${siteId}`, "DELETE", null, "site-status-result", {
      successMessage: `站点 ${siteName} 已删除`,
      refreshAfter: true,
    });
  });

  $("certificate-order-rows").addEventListener("click", async (event) => {
    const detailButton = event.target.closest("[data-order-detail]");
    if (detailButton) {
      closeActionMenu(detailButton);
      const order = state.certificateOrders.find(
        (item) => item.order_id === detailButton.dataset.orderDetail,
      );
      if (order) {
        state.currentCertificateOrderDetail = order;
        renderCertificateDetailPage();
        activateView("certificates", "certificates-detail");
      }
      return;
    }

    const button = event.target.closest("[data-order-action]");
    if (!button) {
      return;
    }
    const orderId = button.dataset.orderId;
    const action = button.dataset.orderAction;
    const path = `/api/admin/certificates/orders/${orderId}/${action}`;
    await submitAction(path, "POST", null, "certificate-order-result", {
      successMessage: `证书订单已${action === "retry" ? "重试" : "重置"}`,
      refreshAfter: true,
    });
  });

  $("release-rows").addEventListener("click", (event) => {
    const detailButton = event.target.closest("[data-release-detail]");
    if (!detailButton) {
      return;
    }
    closeActionMenu(detailButton);
    const release = state.releases.find(
      (item) => item.release_id === detailButton.dataset.releaseDetail,
    );
    if (release) {
      state.currentReleaseDetail = release;
      renderReleaseDetailPage();
      activateView("releases", "releases-detail");
    }
  });

  $("dns-provider-rows").addEventListener("click", (event) => {
    const detailButton = event.target.closest("[data-provider-detail]");
    if (detailButton) {
      closeActionMenu(detailButton);
      const provider = state.dnsProviders.find(
        (item) => item.provider_id === detailButton.dataset.providerDetail,
      );
      if (provider) {
        state.currentDnsProviderDetail = provider;
        renderDnsProviderDetailPage();
        activateView("dns", "dns-provider-detail");
      }
      return;
    }

    const editButton = event.target.closest("[data-provider-edit]");
    if (editButton) {
      closeActionMenu(editButton);
      const provider = state.dnsProviders.find(
        (item) => item.provider_id === editButton.dataset.providerEdit,
      );
      if (provider) {
        hydrateDnsProviderForm(provider);
        activateView("dns", "dns-providers");
      }
      return;
    }

    const deleteButton = event.target.closest("[data-provider-delete]");
    if (!deleteButton) {
      return;
    }
    closeActionMenu(deleteButton);
    const providerId = deleteButton.dataset.providerDelete;
    const providerName = deleteButton.dataset.providerName || providerId;
    if (!window.confirm(`确认删除 Provider ${providerName} 吗？`)) {
      return;
    }
    submitAction(
      `/api/admin/dns/providers/${providerId}`,
      "DELETE",
      null,
      "dns-provider-form-result",
      {
        successMessage: `Provider ${providerName} 已删除`,
        refreshAfter: true,
      },
    );
  });

  $("dns-zone-rows").addEventListener("click", (event) => {
    const detailButton = event.target.closest("[data-zone-detail]");
    if (detailButton) {
      closeActionMenu(detailButton);
      const zone = state.dnsZones.find((item) => item.zone_id === detailButton.dataset.zoneDetail);
      if (zone) {
        state.currentDnsZoneDetail = zone;
        renderDnsZoneDetailPage();
        activateView("dns", "dns-zone-detail");
      }
      return;
    }

    const enableButton = event.target.closest("[data-zone-enable]");
    if (enableButton) {
      closeActionMenu(enableButton);
      submitAction(
        `/api/admin/dns/zones/${enableButton.dataset.zoneEnable}/enable`,
        "POST",
        null,
        "dns-zone-sync-result",
        {
          successMessage: "Zone 已启用",
          refreshAfter: true,
        },
      );
      return;
    }

    const disableButton = event.target.closest("[data-zone-disable]");
    if (disableButton) {
      closeActionMenu(disableButton);
      submitAction(
        `/api/admin/dns/zones/${disableButton.dataset.zoneDisable}/disable`,
        "POST",
        null,
        "dns-zone-sync-result",
        {
          successMessage: "Zone 已禁用",
          refreshAfter: true,
        },
      );
      return;
    }

    const deleteButton = event.target.closest("[data-zone-delete]");
    if (!deleteButton) {
      return;
    }
    closeActionMenu(deleteButton);
    const zoneId = deleteButton.dataset.zoneDelete;
    const zoneName = deleteButton.dataset.zoneName || zoneId;
    if (!window.confirm(`确认删除 Zone ${zoneName} 吗？`)) {
      return;
    }
    submitAction(
      `/api/admin/dns/zones/${zoneId}`,
      "DELETE",
      null,
      "dns-zone-sync-result",
      {
        successMessage: `Zone ${zoneName} 已删除`,
        refreshAfter: true,
      },
    );
  });

  $("operation-template-rows").addEventListener("click", (event) => {
    const detailButton = event.target.closest("[data-template-detail]");
    if (detailButton) {
      closeActionMenu(detailButton);
      const template = state.operationTemplates.find(
        (item) => item.template_id === detailButton.dataset.templateDetail,
      );
      if (template) {
        state.currentOperationTemplateDetail = template;
        renderOperationTemplateDetailPage();
        activateView("nodes", "node-template-detail");
      }
      return;
    }

    const editButton = event.target.closest("[data-template-edit]");
    if (editButton) {
      closeActionMenu(editButton);
      const template = state.operationTemplates.find(
        (item) => item.template_id === editButton.dataset.templateEdit,
      );
      if (template) {
        hydrateOperationTemplateForm(template);
        activateView("nodes", "nodes-templates");
      }
      return;
    }

    const deleteButton = event.target.closest("[data-template-delete]");
    if (!deleteButton) {
      return;
    }
    closeActionMenu(deleteButton);
    const templateId = deleteButton.dataset.templateDelete;
    const templateName = deleteButton.dataset.templateName || templateId;
    if (!window.confirm(`确认删除动作模板 ${templateName} 吗？`)) {
      return;
    }
    submitAction(
      `/api/admin/operations/templates/${templateId}`,
      "DELETE",
      null,
      "operation-template-form-result",
      {
        successMessage: `动作模板 ${templateName} 已删除`,
        refreshAfter: true,
      },
    );
  });
}

async function refreshAll(options = {}) {
  const { silent = false, preserveStatus = false } = options;
  if (!preserveStatus) {
    setConnectionStatus("pending", "连接中");
  }

  try {
    const [
      currentAdmin,
      meta,
      nodes,
      operationTemplates,
      sites,
      releases,
      dnsProviders,
      dnsZones,
      certificateOrders,
    ] =
      await Promise.all([
        apiRequest("/api/admin/auth/me"),
        apiRequest("/api/admin/meta"),
        apiRequest("/api/admin/nodes"),
        apiRequest("/api/admin/operations/templates"),
        apiRequest("/api/admin/sites"),
        apiRequest("/api/admin/releases"),
        apiRequest("/api/admin/dns/providers"),
        apiRequest("/api/admin/dns/zones"),
        apiRequest("/api/admin/certificates/orders"),
      ]);

    state.currentAdmin = currentAdmin?.session || null;
    state.meta = meta;
    state.nodes = nodes;
    state.operationTemplates = operationTemplates;
    state.sites = sites;
    state.releases = releases;
    state.dnsProviders = dnsProviders;
    state.dnsZones = dnsZones;
    state.certificateOrders = certificateOrders;

    renderAll();
    await refreshNodeOperations({ silent: true });
    if (state.currentNodeDetail?.node_id) {
      if (state.nodes.some((node) => node.node_id === state.currentNodeDetail.node_id)) {
        await openNodeDetailDrawer(state.currentNodeDetail.node_id);
      } else {
        closeNodeDetailDrawer();
      }
    }
    if (state.currentSiteDetail?.site_id) {
      if (state.sites.some((site) => site.site_id === state.currentSiteDetail.site_id)) {
        await openSiteDetailDrawer(state.currentSiteDetail.site_id);
      } else {
        closeSiteDetailDrawer();
      }
    }
    $("last-refresh-text").textContent = formatDate(new Date().toISOString());
    setConnectionStatus("connected", "已连接");
    if (!silent) {
      toast("控制台数据已刷新", "success");
    }
  } catch (error) {
    console.error(error);
    setConnectionStatus("error", "连接失败");
    toast(error.message || "刷新控制台失败", "error");
  }
}

function renderAll() {
  renderCurrentAdmin();
  renderMeta();
  renderMetrics();
  renderNodesTable();
  renderNodeMonitor();
  renderNodeDetailDrawer();
  renderOperationTemplatesTable();
  renderNodeOperationsSection();
  renderSitesTable();
  renderSiteDetailDrawer();
  renderReleaseDetailPage();
  renderReleasesTable();
  renderDnsProviderDetailPage();
  renderDnsProviderTable();
  renderDnsZoneDetailPage();
  renderDnsZoneTable();
  renderCertificateDetailPage();
  renderCertificateOrdersTable();
  renderOperationTemplateDetailPage();
  renderDashboardTables();
  renderSelects();
  syncBindingRowOptions();
}

function renderCurrentAdmin() {
  if ($("change-password-username")) {
    $("change-password-username").value = state.currentAdmin
      ? `${state.currentAdmin.display_name} (@${state.currentAdmin.username})`
      : "-";
  }
}

function renderMeta() {
  $("meta-bind").textContent = state.meta?.bind ?? "-";
  $("meta-components").textContent =
    (state.meta?.components || []).join(" / ") || "-";
}

function renderMetrics() {
  const onlineNodes = state.nodes.filter((node) => node.status === "online").length;
  const publishedSites = state.sites.filter((site) => site.status === "published").length;
  const todayReleases = state.releases.filter((release) => isSameLocalDay(release.created_at)).length;
  const pendingReleases = state.releases.filter((release) =>
    ["pending", "publishing"].includes(release.status),
  ).length;
  const failedOrders = state.certificateOrders.filter((order) =>
    ["dns_challenge_failed", "issue_failed"].includes(order.order_status),
  ).length;

  $("metric-site-count").textContent = String(state.sites.length);
  $("metric-site-published").textContent = `已发布 ${publishedSites}`;
  $("metric-node-count").textContent = `${onlineNodes} / ${state.nodes.length}`;
  $("metric-node-online").textContent = `异常 ${Math.max(state.nodes.length - onlineNodes, 0)}`;
  $("metric-release-count").textContent = String(todayReleases || state.releases.length);
  $("metric-release-pending").textContent = `待处理 ${pendingReleases}`;
  $("metric-order-count").textContent = String(state.certificateOrders.length);
  $("metric-order-failed").textContent = `失败 ${failedOrders}`;
}

function renderNodesTable() {
  const rows = state.nodes
    .map(
      (node) => `
        <tr>
          <td>
            <div>${escapeHtml(node.node_code)}</div>
            <div class="mono muted">${escapeHtml(node.node_id)}</div>
          </td>
          <td>${escapeHtml(node.name)}</td>
          <td>${escapeHtml(node.region)} / ${escapeHtml(node.idc)}</td>
          <td>${renderStatusBadge(node.status)}</td>
          <td class="mono">${escapeHtml(node.active_config_version || "-")}</td>
          <td>${formatDate(node.last_seen_at)}</td>
          <td>
            <div class="table-actions">
              <details class="action-menu">
                <summary class="tiny-button menu-button" role="button">操作</summary>
                <div class="action-menu-sheet">
                  <button
                    class="action-menu-item"
                    type="button"
                    data-open-node-detail="${escapeHtml(node.node_id)}"
                  >
                    查看详情
                  </button>
                  <button
                    class="action-menu-item"
                    type="button"
                    data-open-node-operations="${escapeHtml(node.node_id)}"
                  >
                    节点动作
                  </button>
                  <button
                    class="action-menu-item danger"
                    type="button"
                    data-delete-node="${escapeHtml(node.node_id)}"
                    data-node-code="${escapeHtml(node.node_code)}"
                  >
                    删除节点
                  </button>
                </div>
              </details>
            </div>
          </td>
        </tr>
      `,
    )
    .join("");
  $("node-rows").innerHTML = rows || renderEmptyRow("暂无节点数据", 7);
}

function renderNodeMonitor() {
  const snapshots = state.nodes.map((node) => {
    const lagSeconds = computeLagSeconds(node.last_seen_at);
    return {
      ...node,
      lagSeconds,
      host:
        node.labels?.host ||
        node.labels?.private_ip ||
        node.private_ip ||
        node.public_ip ||
        "-",
    };
  });
  const onlineCount = snapshots.filter((node) => node.status === "online").length;
  const anomalyCount = snapshots.filter((node) =>
    ["offline", "suspect", "maintenance"].includes(node.status),
  ).length;
  const staleCount = snapshots.filter(
    (node) => typeof node.lagSeconds === "number" && node.lagSeconds > 90,
  ).length;
  const versionCounts = new Map();
  for (const node of snapshots) {
    const key = node.active_config_version || "未上报配置版本";
    versionCounts.set(key, (versionCounts.get(key) || 0) + 1);
  }

  $("node-monitor-stats").innerHTML = [
    {
      title: "在线节点",
      value: String(onlineCount),
      meta: `${state.nodes.length} 台纳管节点`,
      accent: "accent-blue",
    },
    {
      title: "异常节点",
      value: String(anomalyCount),
      meta: "offline / suspect / maintenance",
      accent: "accent-rose",
    },
    {
      title: "滞后心跳",
      value: String(staleCount),
      meta: "超过 90 秒未上报",
      accent: "accent-amber",
    },
    {
      title: "版本分布",
      value: String(versionCounts.size),
      meta: "当前生效配置版本数",
      accent: "accent-green",
    },
  ]
    .map(
      (card) => `
        <article class="stat-card ${card.accent}">
          <p>${escapeHtml(card.title)}</p>
          <h4>${escapeHtml(card.value)}</h4>
          <span>${escapeHtml(card.meta)}</span>
        </article>
      `,
    )
    .join("");

  $("node-monitor-rows").innerHTML =
    snapshots
      .sort(compareNodeMonitorRows)
      .map(
        (node) => `
          <tr>
            <td>
              <div>${escapeHtml(node.node_code)}</div>
              <div class="mono muted">${escapeHtml(node.node_id)}</div>
            </td>
            <td>
              <div>${escapeHtml(node.host)}</div>
              <div class="muted">${escapeHtml(node.region)} / ${escapeHtml(node.idc)}</div>
            </td>
            <td>${renderStatusBadge(node.status)}</td>
            <td class="mono">${escapeHtml(node.active_config_version || "-")}</td>
            <td>${escapeHtml(formatLag(node.lagSeconds))}</td>
            <td>${formatDate(node.last_seen_at)}</td>
          </tr>
        `,
      )
      .join("") || renderEmptyRow("暂无节点监控数据", 6);

  const sortedVersions = [...versionCounts.entries()].sort((left, right) => right[1] - left[1]);
  $("node-version-stack").innerHTML =
    sortedVersions
      .map(
        ([version, count]) => `
          <div class="info-item">
            <strong>${escapeHtml(version)}</strong>
            <span>${escapeHtml(String(count))} 台节点正在使用这个版本</span>
          </div>
        `,
      )
      .join("") || '<div class="empty-state">暂无版本分布数据</div>';
}

function renderNodeDetailDrawer() {
  const detail = state.currentNodeDetail;
  if (!detail) {
    $("node-detail-title").textContent = "未选择节点";
    $("node-detail-subtitle").textContent = "查看节点当前版本、站点承载和基础运行信息。";
    $("node-detail-runtime").innerHTML = "";
    $("node-detail-overview").innerHTML = '<div class="empty-state">请选择节点查看详情</div>';
    $("node-detail-site-rows").innerHTML = renderEmptyRow("当前节点暂无承载站点", 5);
    return;
  }

  $("node-detail-title").textContent = `${detail.node_code} · ${detail.name}`;
  $("node-detail-subtitle").textContent =
    "聚合节点版本、心跳、站点承载和主机信息，方便在同一个视图里完成判断。";
  $("node-detail-runtime").innerHTML = [
    {
      title: "节点状态",
      value: detail.status || "-",
      meta: `最近心跳 ${formatDate(detail.last_seen_at)}`,
      accent: statusAccent(detail.status),
    },
    {
      title: "当前版本",
      value: detail.active_config_version || "-",
      meta: `Pingora ${detail.pingora_version || "-"} / Agent ${detail.agent_version || "-"}`,
      accent: "accent-blue",
    },
    {
      title: "运行站点",
      value: String(detail.sites?.length || 0),
      meta: `节点上报站点数 ${detail.runtime_site_count ?? 0}`,
      accent: "accent-green",
    },
    {
      title: "健康分",
      value: String(detail.health_score ?? 0),
      meta: "来自最近一次心跳上报",
      accent: "accent-amber",
    },
  ]
    .map(
      (card) => `
        <article class="stat-card ${card.accent}">
          <p>${escapeHtml(card.title)}</p>
          <h4>${escapeHtml(card.value)}</h4>
          <span>${escapeHtml(card.meta)}</span>
        </article>
      `,
    )
    .join("");

  $("node-detail-overview").innerHTML = [
    detailItem("节点编码", detail.node_code),
    detailItem("区域 / IDC", `${detail.region} / ${detail.idc}`),
    detailItem("主机名", detail.hostname || "-"),
    detailItem("IP 地址", buildNodeIpText(detail)),
    detailItem("配置版本", detail.active_config_version || "-"),
    detailItem("上报时间", formatDate(detail.last_seen_at)),
    detailItem("Pingora 版本", detail.pingora_version || "-"),
    detailItem("Agent 版本", detail.agent_version || "-"),
    detailItem("标签", formatJson(detail.labels || {}), true),
  ].join("");

  $("node-detail-site-rows").innerHTML =
    (detail.sites || [])
      .map(
        (site) => `
          <tr>
            <td>
              <div>${escapeHtml(site.site_code)}</div>
              <div class="muted">${escapeHtml(site.name)}</div>
            </td>
            <td>${escapeHtml(site.domain)}</td>
            <td>${renderStatusBadge(site.status)}</td>
            <td>
              <div>${escapeHtml(site.binding_role)}</div>
              <div class="muted">priority ${escapeHtml(String(site.priority))}</div>
            </td>
            <td>v${escapeHtml(String(site.version))}</td>
          </tr>
        `,
      )
      .join("") || renderEmptyRow("当前节点暂无承载站点", 5);
}

function renderSiteDetailDrawer() {
  const detail = state.currentSiteDetail;
  if (!detail) {
    $("site-detail-title").textContent = "未选择站点";
    $("site-detail-subtitle").textContent = "查看站点域名、协议、缓存与绑定信息。";
    $("site-detail-runtime").innerHTML = "";
    $("site-detail-overview").innerHTML = '<div class="empty-state">请选择站点查看详情</div>';
    $("site-detail-binding-rows").innerHTML = renderEmptyRow("当前站点暂无节点绑定", 3);
    return;
  }

  $("site-detail-title").textContent = `${detail.name} · ${detail.domain}`;
  $("site-detail-subtitle").textContent =
    "聚合站点域名、协议、缓存规则、上游配置和绑定关系，方便在同一视图中确认变更。";
  $("site-detail-runtime").innerHTML = [
    {
      title: "站点状态",
      value: detail.status || "-",
      meta: `当前版本 v${detail.version ?? 0}`,
      accent: statusAccent(detail.status),
    },
    {
      title: "协议 / TLS",
      value: `${String(detail.protocol || "-").toUpperCase()} / ${detail.tls_enabled ? "ON" : "OFF"}`,
      meta: `监听端口 ${detail.listen_port || "-"}`,
      accent: "accent-blue",
    },
    {
      title: "上游组",
      value: String(detail.config?.upstreams?.length || 0),
      meta: "当前配置中的 upstream 数量",
      accent: "accent-green",
    },
    {
      title: "缓存规则",
      value: String(detail.config?.cache_rules?.length || 0),
      meta: "当前启用的缓存策略数量",
      accent: "accent-amber",
    },
  ]
    .map(
      (card) => `
        <article class="stat-card ${card.accent}">
          <p>${escapeHtml(card.title)}</p>
          <h4>${escapeHtml(card.value)}</h4>
          <span>${escapeHtml(card.meta)}</span>
        </article>
      `,
    )
    .join("");

  $("site-detail-overview").innerHTML = [
    detailItem("站点名称", detail.name),
    detailItem("站点编码", detail.site_code || "-"),
    detailItem("域名", detail.domain),
    detailItem("协议", String(detail.protocol || "-").toUpperCase()),
    detailItem("监听端口", String(detail.listen_port ?? "-")),
    detailItem("TLS", detail.tls_enabled ? "enabled" : "disabled"),
    detailItem("缓存规则", formatJson(detail.config?.cache_rules || []), true),
    detailItem("Upstreams", formatJson(detail.config?.upstreams || []), true),
  ].join("");

  $("site-detail-binding-rows").innerHTML =
    (detail.bindings || [])
      .map(
        (binding) => `
          <tr>
            <td>${renderBindingNodeCell(binding)}</td>
            <td>${escapeHtml(binding.binding_role)}</td>
            <td>${escapeHtml(String(binding.priority))}</td>
          </tr>
        `,
      )
      .join("") || renderEmptyRow("当前站点暂无节点绑定", 3);
}

function renderReleaseDetailPage() {
  const detail = state.currentReleaseDetail;
  if (!detail) {
    $("release-detail-title").textContent = "未选择发布";
    $("release-detail-subtitle").textContent = "查看发布范围、聚合状态和统计信息。";
    $("release-detail-runtime").innerHTML = "";
    $("release-detail-overview").innerHTML = '<div class="empty-state">请选择发布记录查看详情</div>';
    return;
  }

  const scopeSite = state.sites.find((site) => site.site_id === detail.scope_id);
  const totalTargets =
    (detail.counts?.pending || 0) +
    (detail.counts?.in_progress || 0) +
    (detail.counts?.success || 0) +
    (detail.counts?.failed || 0);
  $("release-detail-title").textContent = detail.release_version;
  $("release-detail-subtitle").textContent = detail.reason || "查看本次发布的聚合状态。";
  $("release-detail-runtime").innerHTML = [
    { title: "状态", value: detail.status, meta: "发布聚合状态", accent: statusAccent(detail.status) },
    {
      title: "范围",
      value: scopeSite?.name || detail.scope_type,
      meta: detail.scope_type === "site" ? scopeSite?.domain || detail.scope_id || "-" : "当前发布范围",
      accent: "accent-blue",
    },
    { title: "成功", value: String(detail.counts?.success || 0), meta: `目标节点 ${totalTargets}`, accent: "accent-green" },
    { title: "失败", value: String(detail.counts?.failed || 0), meta: "失败节点数", accent: "accent-rose" },
  ]
    .map(
      (card) => `
        <article class="stat-card ${card.accent}">
          <p>${escapeHtml(card.title)}</p>
          <h4>${escapeHtml(card.value)}</h4>
          <span>${escapeHtml(card.meta)}</span>
        </article>
      `,
    )
    .join("");
  $("release-detail-overview").innerHTML = [
    detailItem("版本", detail.release_version),
    detailItem("发布类型", detail.release_type),
    detailItem("范围类型", detail.scope_type),
    detailItem("范围站点", scopeSite ? `${scopeSite.name} (${scopeSite.domain})` : "-"),
    detailItem("范围 ID", detail.scope_id || "-"),
    detailItem("创建时间", formatDate(detail.created_at)),
    detailItem("成功节点", String(detail.counts?.success || 0)),
    detailItem("失败节点", String(detail.counts?.failed || 0)),
    detailItem("进行中节点", String(detail.counts?.in_progress || 0)),
    detailItem("待处理节点", String(detail.counts?.pending || 0)),
    detailItem("统计", formatJson(detail.counts || {}), true),
    detailItem("说明", detail.reason || "-", true),
  ].join("");
}

function renderDnsProviderDetailPage() {
  const detail = state.currentDnsProviderDetail;
  if (!detail) {
    $("dns-provider-detail-title").textContent = "未选择 Provider";
    $("dns-provider-detail-subtitle").textContent = "查看 Provider 类型、状态和接入配置摘要。";
    $("dns-provider-detail-overview").innerHTML = '<div class="empty-state">请选择 Provider 查看详情</div>';
    return;
  }

  $("dns-provider-detail-title").textContent = detail.name;
  $("dns-provider-detail-subtitle").textContent =
    "查看 Provider 基础信息，编辑时需补全或替换敏感凭据。";
  $("dns-provider-detail-delete").disabled = detail.status !== "active";
  $("dns-provider-detail-overview").innerHTML = [
    detailItem("Provider 名称", detail.name),
    detailItem("类型", detail.provider_type),
    detailItem("状态", detail.status),
    detailItem("API Endpoint", detail.api_endpoint || "-"),
    detailItem("创建时间", formatDate(detail.created_at)),
    detailItem("提示", "当前接口不会回显凭据明文，编辑时请重新填写 credentials JSON。", true),
  ].join("");
}

function renderDnsZoneDetailPage() {
  const detail = state.currentDnsZoneDetail;
  if (!detail) {
    $("dns-zone-detail-title").textContent = "未选择 Zone";
    $("dns-zone-detail-subtitle").textContent = "查看 Zone 状态、Provider 归属和创建时间。";
    $("dns-zone-detail-overview").innerHTML = '<div class="empty-state">请选择 Zone 查看详情</div>';
    return;
  }

  const provider = state.dnsProviders.find((item) => item.provider_id === detail.provider_id);
  $("dns-zone-detail-title").textContent = detail.zone_name;
  $("dns-zone-detail-subtitle").textContent = "查看当前 Zone 的归属 Provider 和运行状态。";
  $("dns-zone-detail-toggle").textContent = detail.status === "disabled" ? "启用 Zone" : "禁用 Zone";
  $("dns-zone-detail-overview").innerHTML = [
    detailItem("Zone 名称", detail.zone_name),
    detailItem("Zone ID", detail.zone_id),
    detailItem("Provider", provider?.name || detail.provider_id),
    detailItem("状态", detail.status),
    detailItem("创建时间", formatDate(detail.created_at)),
  ].join("");
}

function renderCertificateDetailPage() {
  const detail = state.currentCertificateOrderDetail;
  if (!detail) {
    $("certificate-detail-title").textContent = "未选择订单";
    $("certificate-detail-subtitle").textContent = "查看证书订单状态、ACME 参数和错误信息。";
    $("certificate-detail-overview").innerHTML = '<div class="empty-state">请选择证书订单查看详情</div>';
    return;
  }

  const site = state.sites.find((item) => item.site_id === detail.site_id);
  const zone = state.dnsZones.find((item) => item.zone_id === detail.zone_id);
  const subject = certificateOrderSubject(detail, site);
  const challengeRecordName = detail.challenge_payload?.record_name || "-";
  const challengeRecordValue = detail.challenge_payload?.record_value || "-";
  $("certificate-detail-title").textContent = detail.order_id;
  $("certificate-detail-subtitle").textContent =
    `${subject} · ${detail.acme_provider} · ${detail.order_status}`;
  $("certificate-detail-overview").innerHTML = [
    detailItem("订单 ID", detail.order_id),
    detailItem("证书域名", detail.challenge_payload?.identifier || subject),
    detailItem("关联站点", site?.name || (detail.site_id ? detail.site_id : "独立证书")),
    detailItem("解析区域", zone?.zone_name || detail.zone_id),
    detailItem("订单类型", detail.order_type || "-"),
    detailItem("状态", detail.order_status),
    detailItem("ACME 服务", detail.acme_provider),
    detailItem("验证方式", detail.challenge_type),
    detailItem("证书 ID", detail.certificate_id || "-"),
    detailItem("证书到期时间", formatDate(detail.certificate_expires_at)),
    detailItem("下次续签时间", formatDate(detail.next_renew_at)),
    detailItem("创建时间", formatDate(detail.created_at)),
    detailItem("Challenge Record", challengeRecordName, true),
    detailItem("Challenge Value", challengeRecordValue, true),
    detailItem("错误信息", detail.error_message || "-", true),
    detailItem("Challenge Payload", formatJson(detail.challenge_payload || {}), true),
  ].join("");

  $("certificate-detail-retry").disabled = !["dns_challenge_failed", "issue_failed"].includes(
    detail.order_status,
  );
  $("certificate-detail-reset").disabled = detail.order_status === "issued";
}

function renderOperationTemplateDetailPage() {
  const detail = state.currentOperationTemplateDetail;
  if (!detail) {
    $("operation-template-detail-title").textContent = "未选择模板";
    $("operation-template-detail-subtitle").textContent =
      "查看模板的执行规则、参数白名单和编辑入口。";
    $("operation-template-detail-overview").innerHTML =
      '<div class="empty-state">请选择动作模板查看详情</div>';
    return;
  }

  $("operation-template-detail-title").textContent = detail.name;
  $("operation-template-detail-subtitle").textContent =
    "查看模板类型、运行用户、审批要求和允许参数。";
  $("operation-template-detail-overview").innerHTML = [
    detailItem("模板名称", detail.name),
    detailItem("模板 ID", detail.template_id),
    detailItem("类型", detail.operation_type),
    detailItem("运行用户", detail.run_as_user),
    detailItem("超时", `${detail.timeout_seconds}s`),
    detailItem("审批", detail.approval_required ? "required" : "not required"),
    detailItem("允许参数", (detail.allowed_params || []).join(", ") || "-", true),
    detailItem("命令模板", detail.command_template || "-", true),
  ].join("");
}

function renderOperationTemplatesTable() {
  const rows = state.operationTemplates
    .map(
      (template) => `
        <tr>
          <td>
            <div>${escapeHtml(template.name)}</div>
            <div class="mono muted">${escapeHtml(template.template_id)}</div>
          </td>
          <td>${escapeHtml(template.operation_type)}</td>
          <td>${escapeHtml(template.run_as_user)}</td>
          <td>${escapeHtml(String(template.timeout_seconds))}s</td>
          <td>${renderStatusBadge(template.approval_required ? "pending" : "active")}</td>
          <td>${escapeHtml((template.allowed_params || []).join(", ") || "-")}</td>
          <td>
            <details class="action-menu">
              <summary>
                <button class="tiny-button menu-button" type="button">操作</button>
              </summary>
              <div class="action-menu-sheet">
                <button
                  class="action-menu-item"
                  type="button"
                  data-template-detail="${escapeHtml(template.template_id)}"
                >
                  查看详情
                </button>
                <button
                  class="action-menu-item"
                  type="button"
                  data-template-edit="${escapeHtml(template.template_id)}"
                >
                  编辑模板
                </button>
                <button
                  class="action-menu-item danger"
                  type="button"
                  data-template-delete="${escapeHtml(template.template_id)}"
                  data-template-name="${escapeHtml(template.name)}"
                >
                  删除模板
                </button>
              </div>
            </details>
          </td>
        </tr>
      `,
    )
    .join("");
  $("operation-template-rows").innerHTML = rows || renderEmptyRow("暂无动作模板", 7);
}

function renderNodeOperationsSection() {
  const selectedNode = state.nodes.find(
    (node) => node.node_id === state.selectedNodeOperationNodeId,
  );
  const nodeName = selectedNode
    ? `${selectedNode.node_code} · ${selectedNode.name}`
    : "尚未选择节点";
  const stats = summarizeNodeOperations(state.nodeOperations);

  $("node-operation-current-title").textContent = nodeName;
  $("node-operation-current-meta").textContent = selectedNode
    ? `当前查看 ${selectedNode.node_code} 的动作任务，系统切换和人工下发都会汇总到这里。`
    : "请选择节点后查看该节点的动作统计和执行结果。";

  $("node-operation-stats").innerHTML = [
    {
      title: "待执行",
      value: String(stats.pending + stats.approved),
      meta: "pending / approved",
      accent: "accent-amber",
    },
    {
      title: "运行中",
      value: String(stats.running),
      meta: "节点正在执行中的动作",
      accent: "accent-blue",
    },
    {
      title: "成功",
      value: String(stats.success),
      meta: "最近拉取并执行成功的任务",
      accent: "accent-green",
    },
    {
      title: "失败",
      value: String(stats.failed + stats.timeout + stats.cancelled),
      meta: "failed / timeout / cancelled",
      accent: "accent-rose",
    },
  ]
    .map(
      (card) => `
        <article class="stat-card ${card.accent}">
          <p>${escapeHtml(card.title)}</p>
          <h4>${escapeHtml(card.value)}</h4>
          <span>${escapeHtml(card.meta)}</span>
        </article>
      `,
    )
    .join("");

  const rows = state.nodeOperations
    .map(
      (operation) => `
        <tr>
          <td>${formatDate(operation.created_at)}</td>
          <td>
            <div>${escapeHtml(operation.template_name)}</div>
            <div class="muted">${escapeHtml(operation.operation_type)}</div>
          </td>
          <td>${renderStatusBadge(operation.exec_status)}</td>
          <td>
            <div>${escapeHtml(operation.requested_by)}</div>
            <div class="muted">${escapeHtml(operation.approved_by || "-")}</div>
          </td>
          <td>
            <div>${escapeHtml(summarizeOperationOutput(operation))}</div>
            <div class="muted">${escapeHtml(formatDate(operation.finished_at))}</div>
          </td>
        </tr>
      `,
    )
    .join("");
  $("node-operation-rows").innerHTML =
    rows || renderEmptyRow("当前节点暂无动作记录", 5);
}

function renderPrimaryNodeSummary(site) {
  if (!site.primary_node_id) {
    return '<span class="muted">未绑定主节点</span>';
  }
  const label = [site.primary_node_code, site.primary_node_name].filter(Boolean).join(" · ");
  return `
    <div>${escapeHtml(label || site.primary_node_id)}</div>
    <div class="muted">${renderStatusBadge(site.primary_node_status || "unknown")}</div>
  `;
}

function renderBindingNodeCell(binding) {
  const label = [binding.node_code, binding.node_name].filter(Boolean).join(" · ");
  const status = binding.node_status ? renderStatusBadge(binding.node_status) : "";
  return `
    <div>${escapeHtml(label || binding.node_id)}</div>
    <div class="mono muted">${escapeHtml(binding.node_id)}</div>
    ${status ? `<div>${status}</div>` : ""}
  `;
}

function renderSiteRowActions(site) {
  const disabledSwitch = Number(site.binding_count || 0) < 2;
  const statusAction =
    site.status === "disabled"
      ? `<button class="action-chip" type="button" data-enable-site="${escapeHtml(
          site.site_id,
        )}">启用</button>`
      : `<button class="action-chip" type="button" data-disable-site="${escapeHtml(
          site.site_id,
        )}">禁用</button>`;

  return `
    <div class="site-row-actions">
      <button class="action-chip primary" type="button" data-open-site-detail="${escapeHtml(
        site.site_id,
      )}">详情</button>
      <button class="action-chip" type="button" data-edit-site="${escapeHtml(
        site.site_id,
      )}">编辑</button>
      <button class="action-chip" type="button" data-edit-site-bindings="${escapeHtml(
        site.site_id,
      )}">绑定</button>
      <button class="action-chip" type="button" data-renew-site-certificate="${escapeHtml(
        site.site_id,
      )}">证书</button>
      <button
        class="action-chip"
        type="button"
        data-switch-site-primary="${escapeHtml(site.site_id)}"
        ${disabledSwitch ? "disabled" : ""}
        title="${escapeHtml(
          disabledSwitch ? "至少需要一个备节点后才能一键切换" : "切换到当前站点已绑定的备用节点",
        )}"
      >切主</button>
      ${statusAction}
      <button
        class="action-chip danger"
        type="button"
        data-delete-site="${escapeHtml(site.site_id)}"
        data-site-name="${escapeHtml(site.name || site.site_code)}"
      >删除</button>
    </div>
  `;
}

function renderSitesTable() {
  const rows = state.sites
    .map(
      (site) => `
        <tr>
          <td>${escapeHtml(site.name)}</td>
          <td>${escapeHtml(site.domain)}</td>
          <td>${escapeHtml(String(site.protocol || "-").toUpperCase())}</td>
          <td>${renderStatusBadge(site.status)}</td>
          <td>v${escapeHtml(String(site.version))}</td>
          <td>
            <div>${renderPrimaryNodeSummary(site)}</div>
            <div class="muted">${escapeHtml(String(site.binding_count))} 个绑定</div>
          </td>
          <td>
            ${renderSiteRowActions(site)}
          </td>
        </tr>
      `,
    )
    .join("");
  $("site-rows").innerHTML = rows || renderEmptyRow("暂无站点数据", 7);
}

function renderReleasesTable() {
  const rows = state.releases
    .map(
      (release) => `
        <tr>
          <td>
            <div>${escapeHtml(release.release_version)}</div>
            <div class="mono muted">${escapeHtml(release.release_id)}</div>
          </td>
          <td>${renderStatusBadge(release.status)}</td>
          <td>${escapeHtml(release.scope_type)}</td>
          <td>${renderReleaseCounts(release.counts)}</td>
          <td>${escapeHtml(release.reason)}</td>
          <td>${formatDate(release.created_at)}</td>
          <td>
            <details class="action-menu">
              <summary>
                <button class="tiny-button menu-button" type="button">操作</button>
              </summary>
              <div class="action-menu-sheet">
                <button
                  class="action-menu-item"
                  type="button"
                  data-release-detail="${escapeHtml(release.release_id)}"
                >
                  查看详情
                </button>
              </div>
            </details>
          </td>
        </tr>
      `,
    )
    .join("");
  $("release-rows").innerHTML = rows || renderEmptyRow("暂无发布记录", 7);
}

function renderDnsProviderTable() {
  const rows = state.dnsProviders
    .map(
      (provider) => `
        <tr>
          <td>
            <div>${escapeHtml(provider.name)}</div>
            <div class="mono muted">${escapeHtml(provider.provider_id)}</div>
          </td>
          <td>${escapeHtml(provider.provider_type)}</td>
          <td>${renderStatusBadge(provider.status)}</td>
          <td>${formatDate(provider.created_at)}</td>
          <td>
            <details class="action-menu">
              <summary>
                <button class="tiny-button menu-button" type="button">操作</button>
              </summary>
              <div class="action-menu-sheet">
                <button
                  class="action-menu-item"
                  type="button"
                  data-provider-detail="${escapeHtml(provider.provider_id)}"
                >
                  查看详情
                </button>
                <button
                  class="action-menu-item"
                  type="button"
                  data-provider-edit="${escapeHtml(provider.provider_id)}"
                >
                  编辑 Provider
                </button>
                <button
                  class="action-menu-item danger"
                  type="button"
                  data-provider-delete="${escapeHtml(provider.provider_id)}"
                  data-provider-name="${escapeHtml(provider.name)}"
                >
                  删除 Provider
                </button>
              </div>
            </details>
          </td>
        </tr>
      `,
    )
    .join("");
  $("dns-provider-rows").innerHTML = rows || renderEmptyRow("暂无 DNS Provider", 5);
}

function renderDnsZoneTable() {
  const providerMap = new Map(
    state.dnsProviders.map((provider) => [provider.provider_id, provider.name]),
  );
  const rows = state.dnsZones
    .map(
      (zone) => `
        <tr>
          <td>
            <div>${escapeHtml(zone.zone_name)}</div>
            <div class="mono muted">${escapeHtml(zone.zone_id)}</div>
          </td>
          <td>${escapeHtml(providerMap.get(zone.provider_id) || zone.provider_id)}</td>
          <td>${renderStatusBadge(zone.status)}</td>
          <td>${formatDate(zone.created_at)}</td>
          <td>
            <details class="action-menu">
              <summary>
                <button class="tiny-button menu-button" type="button">操作</button>
              </summary>
              <div class="action-menu-sheet">
                <button
                  class="action-menu-item"
                  type="button"
                  data-zone-detail="${escapeHtml(zone.zone_id)}"
                >
                  查看详情
                </button>
                ${
                  zone.status === "disabled"
                    ? `
                <button
                  class="action-menu-item"
                  type="button"
                  data-zone-enable="${escapeHtml(zone.zone_id)}"
                >
                  启用 Zone
                </button>`
                    : `
                <button
                  class="action-menu-item"
                  type="button"
                  data-zone-disable="${escapeHtml(zone.zone_id)}"
                >
                  禁用 Zone
                </button>`
                }
                <button
                  class="action-menu-item danger"
                  type="button"
                  data-zone-delete="${escapeHtml(zone.zone_id)}"
                  data-zone-name="${escapeHtml(zone.zone_name)}"
                >
                  删除 Zone
                </button>
              </div>
            </details>
          </td>
        </tr>
      `,
    )
    .join("");
  $("dns-zone-rows").innerHTML = rows || renderEmptyRow("暂无 DNS Zone", 5);
}

function renderCertificateOrdersTable() {
  const rows = state.certificateOrders
    .map((order) => {
      const site = order.site_id ? state.sites.find((item) => item.site_id === order.site_id) : null;
      const subject = certificateOrderSubject(order, site);
      const actions = [];
      if (["dns_challenge_failed", "issue_failed"].includes(order.order_status)) {
        actions.push(
          `<button class="tiny-button" type="button" data-order-action="retry" data-order-id="${escapeHtml(
            order.order_id,
          )}">重试</button>`,
        );
      }
      if (order.order_status !== "issued") {
        actions.push(
          `<button class="tiny-button danger" type="button" data-order-action="reset" data-order-id="${escapeHtml(
            order.order_id,
          )}">重置</button>`,
        );
      }
      return `
        <tr>
          <td>
            <div>${escapeHtml(subject)}</div>
            <div class="mono muted">${escapeHtml(order.order_id)}</div>
          </td>
          <td>${renderStatusBadge(order.order_status)}</td>
          <td>${escapeHtml(order.acme_provider)}</td>
          <td>${escapeHtml(order.error_message || "-")}</td>
          <td>${formatDate(order.created_at)}</td>
          <td>${formatDate(order.next_renew_at)}</td>
          <td>
            <details class="action-menu">
              <summary>
                <button class="tiny-button menu-button" type="button">操作</button>
              </summary>
              <div class="action-menu-sheet">
                <button
                  class="action-menu-item"
                  type="button"
                  data-order-detail="${escapeHtml(order.order_id)}"
                >
                  查看详情
                </button>
                ${actions.join("") || '<span class="muted">暂无操作</span>'}
              </div>
            </details>
          </td>
        </tr>
      `;
    })
    .join("");
  $("certificate-order-rows").innerHTML =
    rows || renderEmptyRow("暂无证书订单", 7);
}

function renderDashboardTables() {
  const siteMap = new Map(state.sites.map((site) => [site.site_id, site.name || site.site_code]));
  $("dashboard-site-rows").innerHTML =
    state.sites
      .slice(0, 6)
      .map((site) => {
        const isDisabled = site.status === "disabled";
        return `
          <tr>
            <td>
              <div>${escapeHtml(site.name)}</div>
              <div class="mono muted">${escapeHtml(site.site_code)}</div>
            </td>
            <td>${escapeHtml(site.domain)}</td>
            <td>${renderStatusBadge(site.status)}</td>
            <td>${renderPrimaryNodeSummary(site)}</td>
            <td>
              <details class="action-menu">
                <summary>
                  <button class="tiny-button menu-button" type="button">操作</button>
                </summary>
                <div class="action-menu-sheet">
                  <button
                    class="action-menu-item"
                    type="button"
                    data-open-site-detail="${escapeHtml(site.site_id)}"
                  >
                    详情
                  </button>
                  <button
                    class="action-menu-item"
                    type="button"
                    data-edit-site-config="${escapeHtml(site.site_id)}"
                  >
                    编辑
                  </button>
                  ${
                    isDisabled
                      ? `<button class="action-menu-item" type="button" data-enable-site="${escapeHtml(
                          site.site_id,
                        )}">启用</button>`
                      : `<button class="action-menu-item" type="button" data-disable-site="${escapeHtml(
                          site.site_id,
                        )}">禁用</button>`
                  }
                  <button
                    class="action-menu-item danger"
                    type="button"
                    data-delete-site="${escapeHtml(site.site_id)}"
                    data-site-name="${escapeHtml(site.name || site.site_code)}"
                  >
                    删除
                  </button>
                </div>
              </details>
            </td>
          </tr>
        `;
      })
      .join("") || renderEmptyRow("暂无站点数据", 5);

  renderDashboardInsights(siteMap);
  renderDashboardTrend();
}

function renderDashboardInsights(siteMap) {
  const latestSuccessRelease = state.releases.find((release) => release.status === "success");
  const latestRelease = latestSuccessRelease || state.releases[0];
  const latestOrder = state.certificateOrders[0];
  const failedOrders = state.certificateOrders.filter((order) =>
    ["dns_challenge_failed", "issue_failed"].includes(order.order_status),
  );
  const unhealthyNodes = state.nodes.filter((node) => node.status !== "online");
  const syncedZones = state.dnsZones.filter((zone) => zone.status !== "disabled").length;

  const insights = [
    {
      tone: latestSuccessRelease ? "success" : "warning",
      iconClass: "icon-release",
      title: latestRelease
        ? `最近发布 ${latestRelease.release_version}`
        : "最近发布等待开始",
      copy: latestRelease
        ? `${latestRelease.reason || "配置发布"} · ${formatDate(latestRelease.created_at)}`
        : "创建站点配置后，发布任务会在这里形成时间线。",
    },
    {
      tone: unhealthyNodes.length > 0 ? "warning" : "success",
      iconClass: "icon-node",
      title: unhealthyNodes.length > 0 ? `节点异常 ${unhealthyNodes.length} 台` : "节点健康检查正常",
      copy:
        unhealthyNodes.length > 0
          ? unhealthyNodes
              .slice(0, 3)
              .map((node) => `${node.node_code || node.name}: ${node.status}`)
              .join(" / ")
          : `${state.nodes.length} 台纳管节点处于可观测状态。`,
    },
    {
      tone: syncedZones > 0 ? "success" : "warning",
      iconClass: "icon-dns",
      title: `DNS 同步状态 ${syncedZones} / ${state.dnsZones.length}`,
      copy: state.dnsProviders.length
        ? `${state.dnsProviders.length} 个 Provider 已接入，Zone 可用于证书和解析自动化。`
        : "接入 DNS Provider 后可启用 Zone 同步。",
    },
    {
      tone: failedOrders.length > 0 ? "danger" : "success",
      iconClass: "icon-cert",
      title: failedOrders.length > 0 ? `证书异常 ${failedOrders.length} 单` : "证书续期状态平稳",
      copy: latestOrder
        ? `${certificateOrderSubject(
            latestOrder,
            latestOrder.site_id
              ? state.sites.find((site) => site.site_id === latestOrder.site_id)
              : null,
          )} · ${latestOrder.order_status}`
        : "还没有证书订单，申请后会显示 challenge 和签发状态。",
    },
  ];

  $("dashboard-insight-list").innerHTML = insights
    .map(
      (item) => `
        <article class="insight-item ${item.tone}">
          <div class="insight-icon ${escapeHtml(item.iconClass)}"></div>
          <div>
            <strong class="insight-title">${escapeHtml(item.title)}</strong>
            <p class="insight-copy">${escapeHtml(item.copy)}</p>
          </div>
        </article>
      `,
    )
    .join("");
}

function renderDashboardTrend() {
  const buckets = [];
  const now = new Date();
  for (let offset = 6; offset >= 0; offset -= 1) {
    const day = new Date(now);
    day.setDate(now.getDate() - offset);
    day.setHours(0, 0, 0, 0);
    buckets.push({
      day,
      label: `${String(day.getMonth() + 1).padStart(2, "0")}-${String(day.getDate()).padStart(
        2,
        "0",
      )}`,
      count: 0,
    });
  }

  for (const release of state.releases) {
    const createdAt = new Date(release.created_at);
    if (Number.isNaN(createdAt.getTime())) {
      continue;
    }
    const bucket = buckets.find(
      (item) =>
        item.day.getFullYear() === createdAt.getFullYear() &&
        item.day.getMonth() === createdAt.getMonth() &&
        item.day.getDate() === createdAt.getDate(),
    );
    if (bucket) {
      bucket.count += 1;
    }
  }

  const maxCount = Math.max(...buckets.map((bucket) => bucket.count), 1);
  $("dashboard-trend-title").textContent = `7 日发布趋势 · ${buckets.reduce(
    (sum, bucket) => sum + bucket.count,
    0,
  )} 次`;
  $("dashboard-trend").innerHTML = buckets
    .map((bucket) => {
      const ratio = bucket.count / maxCount;
      const height = 8 + ratio * 118;
      const pointY = ratio * 118;
      return `
        <div class="chart-day" title="${escapeHtml(bucket.label)} · ${bucket.count} 次发布">
          <div class="chart-point" style="--point-y: ${pointY}px"></div>
          <div class="chart-bar" style="height: ${height}px"></div>
          <span class="chart-label">${escapeHtml(bucket.label)}</span>
        </div>
      `;
    })
    .join("");
}

function renderSelects() {
  setSelectOptions(
    $("binding-site-id"),
    state.sites,
    (site) => site.site_id,
    (site) => `${site.site_code} · ${site.domain}`,
    "请选择站点",
  );
  setSelectOptions(
    $("release-site-id"),
    state.sites,
    (site) => site.site_id,
    (site) => `${site.site_code} · ${site.domain}`,
    "不关联站点",
  );
  setSelectOptions(
    $("certificate-site-id"),
    state.sites,
    (site) => site.site_id,
    (site) => `${site.site_code} · ${site.domain}`,
    "请选择站点",
  );
  setSelectOptions(
    $("dns-provider-id"),
    state.dnsProviders,
    (provider) => provider.provider_id,
    (provider) => `${provider.name} · ${provider.provider_type}`,
    "请选择 Provider",
  );
  setSelectOptions(
    $("certificate-zone-id"),
    state.dnsZones,
    (zone) => zone.zone_id,
    (zone) => `${zone.zone_name} · ${zone.status}`,
    "请选择 Zone",
  );
  setSelectOptions(
    $("node-operation-node-id"),
    state.nodes,
    (node) => node.node_id,
    (node) => `${node.node_code} · ${node.name}`,
    "请选择节点",
  );
  setSelectOptions(
    $("node-operation-template-id"),
    state.operationTemplates,
    (template) => template.template_id,
    (template) => `${template.name} · ${template.operation_type}`,
    "请选择动作模板",
  );
  syncNodeOperationSelection();
}

function setSelectOptions(element, items, valueFn, labelFn, placeholder) {
  const previousValue = element.value;
  const optionMarkup = items
    .map((item) => {
      const value = String(valueFn(item));
      return `<option value="${escapeHtml(value)}">${escapeHtml(labelFn(item))}</option>`;
    })
    .join("");
  element.innerHTML =
    `<option value="">${escapeHtml(placeholder)}</option>` + optionMarkup;
  if (items.some((item) => String(valueFn(item)) === previousValue)) {
    element.value = previousValue;
  }
}

function syncNodeOperationSelection() {
  const nodeSelect = $("node-operation-node-id");
  const currentValue =
    state.selectedNodeOperationNodeId ||
    nodeSelect.value ||
    state.nodes[0]?.node_id ||
    "";
  if (currentValue && state.nodes.some((node) => node.node_id === currentValue)) {
    nodeSelect.value = currentValue;
    state.selectedNodeOperationNodeId = currentValue;
  } else {
    nodeSelect.value = "";
    state.selectedNodeOperationNodeId = null;
    state.nodeOperations = [];
  }
}

function ensureBindingRow() {
  if ($("binding-rows").children.length === 0) {
    addBindingRow();
  }
}

function addBindingRow(initial = {}) {
  const row = document.createElement("div");
  row.className = "binding-row";
  row.innerHTML = `
    <select data-binding-field="node_id"></select>
    <select data-binding-field="binding_role">
      <option value="primary">primary</option>
      <option value="standby">standby</option>
    </select>
    <input data-binding-field="priority" type="number" value="${escapeHtml(
      String(initial.priority ?? 100),
    )}" />
    <button class="ghost-button" type="button" data-remove-binding title="移除">-</button>
  `;
  $("binding-rows").appendChild(row);
  syncBindingRowOptions();
  row.querySelector('[data-binding-field="node_id"]').value = initial.node_id || "";
  row.querySelector('[data-binding-field="binding_role"]').value =
    ["primary", "standby"].includes(initial.binding_role) ? initial.binding_role : "primary";
}

function syncBindingRowOptions() {
  for (const select of document.querySelectorAll('[data-binding-field="node_id"]')) {
    const currentValue = select.value;
    select.innerHTML =
      `<option value="">请选择节点</option>` +
      state.nodes
        .map(
          (node) =>
            `<option value="${escapeHtml(node.node_id)}">${escapeHtml(
              `${node.node_code} · ${node.name}`,
            )}</option>`,
        )
        .join("");
    if (state.nodes.some((node) => node.node_id === currentValue)) {
      select.value = currentValue;
    }
  }
}

async function handleNodeCreate(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const payload = {
    node_code: form.node_code.value.trim(),
    name: form.name.value.trim(),
    region: form.region.value.trim(),
    idc: form.idc.value.trim(),
    labels: parseJsonInput(form.labels.value, {}),
  };
  await submitAction("/api/admin/nodes", "POST", payload, "node-form-result", {
    successMessage: "节点已创建，可继续用于节点注册",
    refreshAfter: true,
    resetForm: form,
  });
  form.labels.value = '{"role":"edge","tier":"primary"}';
  activateView("nodes", "nodes-create");
}

async function handleSiteSubmit(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const siteId = $("site-id").value;
  const baseConfig = parseJsonInput(form.config.value, {});
  const mergedConfig = mergeManagedSiteConfig(baseConfig, form);
  form.config.value = formatJson(mergedConfig);
  const payload = {
    name: form.name.value.trim(),
    domain: form.domain.value.trim(),
    listen_port: Number(form.listen_port.value),
    protocol: form.protocol.value,
    tls_enabled: form.tls_enabled.checked,
    config: mergedConfig,
  };

  let path = "/api/admin/sites";
  let method = "POST";
  if (siteId) {
    path = `/api/admin/sites/${siteId}`;
    method = "PUT";
  }

  const response = await submitAction(path, method, payload, "site-form-result", {
    successMessage: siteId ? "站点已更新" : "站点已创建",
    refreshAfter: true,
  });
  if (!siteId) {
    resetSiteForm();
  } else if (response?.site_id) {
    await loadSiteIntoForm(response.site_id, {
      toastMessage: false,
      subsection: "sites-config",
    });
  }
  activateView("sites", "sites-config");
}

async function handleBindingsSubmit(event) {
  event.preventDefault();
  const siteId = $("binding-site-id").value;
  const bindings = [...document.querySelectorAll(".binding-row")]
    .map((row) => ({
      node_id: row.querySelector('[data-binding-field="node_id"]').value,
      binding_role: row.querySelector('[data-binding-field="binding_role"]').value,
      priority: Number(row.querySelector('[data-binding-field="priority"]').value || 100),
    }))
    .filter((binding) => binding.node_id);

  if (!siteId) {
    throwFormError("binding-form-result", "请先选择站点");
    return;
  }
  if (bindings.length === 0) {
    throwFormError("binding-form-result", "请至少配置一个节点绑定");
    return;
  }

  await submitAction(
    `/api/admin/sites/${siteId}/bindings`,
    "POST",
    { bindings },
    "binding-form-result",
    {
      successMessage: "站点绑定已保存",
      refreshAfter: true,
    },
  );
  activateView("sites", "sites-bindings");
}

async function handleReleaseSubmit(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const payload = {
    scope_type: "site",
    scope_id: form.scope_id.value,
    release_type: form.release_type.value,
    reason: form.reason.value.trim(),
  };
  await submitAction("/api/admin/releases", "POST", payload, "release-form-result", {
    successMessage: "发布任务已创建",
    refreshAfter: true,
    resetForm: form,
  });
  activateView("releases", "releases-records");
}

async function handleDnsProviderSubmit(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const providerId = $("dns-provider-id-hidden").value;
  const payload = {
    name: form.name.value.trim(),
    provider_type: form.provider_type.value,
    api_endpoint: form.api_endpoint.value.trim() || null,
    credentials: parseJsonInput(form.credentials.value, {}),
  };
  const path = providerId ? `/api/admin/dns/providers/${providerId}` : "/api/admin/dns/providers";
  const method = providerId ? "PUT" : "POST";
  await submitAction(
    path,
    method,
    payload,
    "dns-provider-form-result",
    {
      successMessage: providerId ? "DNS Provider 已更新" : "DNS Provider 已保存",
      refreshAfter: true,
      resetForm: form,
    },
  );
  $("dns-provider-id-hidden").value = "";
  form.credentials.value = '{"api_token":"replace-with-token"}';
  activateView("dns", "dns-providers");
}

async function handleDnsZoneSyncSubmit(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const zoneNames = form.zone_names.value
    .split(/[\n,]+/)
    .map((item) => item.trim())
    .filter(Boolean);
  const payload = {
    provider_id: form.provider_id.value,
    zone_names: zoneNames,
  };
  await submitAction(
    "/api/admin/dns/zones/sync",
    "POST",
    payload,
    "dns-zone-sync-result",
    {
      successMessage: "DNS Zone 已同步",
      refreshAfter: true,
    },
  );
  activateView("dns", "dns-zones");
}

async function handleCertificateOrderSubmit(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const payload = {
    site_id: form.site_id.value || null,
    domain: form.domain.value.trim() || null,
    zone_id: form.zone_id.value,
    acme_provider: form.acme_provider.value,
    challenge_type: form.challenge_type.value,
  };
  await submitAction(
    "/api/admin/certificates/orders",
    "POST",
    payload,
    "certificate-order-result",
    {
      successMessage: "证书订单已创建",
      refreshAfter: true,
    },
  );
  activateView("certificates", "certificates-orders");
}

async function handleOperationTemplateSubmit(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const templateId = $("operation-template-id-hidden").value;
  const payload = {
    name: form.name.value.trim(),
    operation_type: form.operation_type.value,
    command_template: form.command_template.value.trim(),
    allowed_params: parseCommaList(form.allowed_params.value),
    timeout_seconds: Number(form.timeout_seconds.value || 60),
    run_as_user: form.run_as_user.value.trim() || "root",
    approval_required: form.approval_required.checked,
  };
  const path = templateId
    ? `/api/admin/operations/templates/${templateId}`
    : "/api/admin/operations/templates";
  const method = templateId ? "PUT" : "POST";
  await submitAction(
    path,
    method,
    payload,
    "operation-template-form-result",
    {
      successMessage: templateId
        ? `动作模板 ${payload.name} 已更新`
        : `动作模板 ${payload.name} 已保存`,
      refreshAfter: true,
    },
  );
  $("operation-template-id-hidden").value = "";
  activateView("nodes", "nodes-templates");
}

async function handleNodeOperationSubmit(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const nodeId = form.node_id.value;
  if (!nodeId) {
    throwFormError("node-operation-form-result", "请先选择目标节点");
    return;
  }
  const payload = {
    template_id: form.template_id.value,
    input_params: parseJsonInput(form.input_params.value, {}),
    approval_ticket: form.approval_ticket.value.trim() || null,
  };
  await submitAction(
    `/api/admin/nodes/${nodeId}/operations`,
    "POST",
    payload,
    "node-operation-form-result",
    {
      successMessage: "节点动作已创建",
      refreshAfter: true,
    },
  );
  state.selectedNodeOperationNodeId = nodeId;
  await refreshNodeOperations({ silent: true });
  activateView("nodes", "nodes-operations");
}

async function handleNodeOperationNodeChange(event) {
  state.selectedNodeOperationNodeId = event.currentTarget.value || null;
  await refreshNodeOperations({ silent: true });
}

async function handleChangePasswordSubmit(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const currentPassword = form.current_password.value;
  const newPassword = form.new_password.value;
  const confirmPassword = form.confirm_password.value;

  if (newPassword !== confirmPassword) {
    throwFormError("change-password-result", "两次输入的新密码不一致");
    return;
  }

  if (newPassword.length < 6) {
    throwFormError("change-password-result", "新密码至少需要 6 位");
    return;
  }

  await submitAction(
    "/api/admin/auth/change-password",
    "POST",
    {
      current_password: currentPassword,
      new_password: newPassword,
    },
    "change-password-result",
    {
      successMessage: "管理员密码已更新，其他会话已失效",
      resetForm: form,
      refreshAfter: true,
    },
  );
  activateView("account", "account-security");
}

async function handleLogout() {
  try {
    await apiRequest("/api/admin/auth/logout", { method: "POST" });
  } catch (_error) {
    // Ignore logout API errors and force back to login.
  }
  window.location.assign("/login");
}

async function submitAction(path, method, payload, resultId, options = {}) {
  const { successMessage, refreshAfter = false, resetForm = null, toastMessage = true } =
    options;
  try {
    const response = await apiRequest(path, { method, body: payload });
    setResultBox(resultId, response);
    if (resetForm) {
      resetForm.reset();
    }
    if (refreshAfter) {
      await refreshAll({ silent: true, preserveStatus: true });
    }
    if (successMessage && toastMessage) {
      toast(successMessage, "success");
    }
    return response;
  } catch (error) {
    setResultBox(resultId, { error: error.message });
    toast(error.message || "操作失败", "error");
    throw error;
  }
}

async function refreshNodeOperations(options = {}) {
  const { silent = false } = options;
  const nodeId = state.selectedNodeOperationNodeId;
  if (!nodeId) {
    state.nodeOperations = [];
    renderNodeOperationsSection();
    return;
  }

  try {
    state.nodeOperations = await apiRequest(`/api/admin/nodes/${nodeId}/operations`);
    renderNodeOperationsSection();
  } catch (error) {
    state.nodeOperations = [];
    renderNodeOperationsSection();
    if (!silent) {
      toast(error.message || "加载节点动作失败", "error");
    }
  }
}

async function openNodeDetailDrawer(nodeId) {
  if (!nodeId) {
    return;
  }
  try {
    state.currentNodeDetail = await apiRequest(`/api/admin/nodes/${nodeId}`);
    renderNodeDetailDrawer();
    activateView("nodes", "nodes-detail");
  } catch (error) {
    toast(error.message || "加载节点详情失败", "error");
  }
}

function closeNodeDetailDrawer() {
  state.currentNodeDetail = null;
  renderNodeDetailDrawer();
  activateView("nodes", "nodes-status");
}

async function openSiteDetailDrawer(siteId) {
  if (!siteId) {
    return;
  }
  try {
    state.currentSiteDetail = await apiRequest(`/api/admin/sites/${siteId}`);
    renderSiteDetailDrawer();
    activateView("sites", "sites-detail");
  } catch (error) {
    toast(error.message || "加载站点详情失败", "error");
  }
}

function closeSiteDetailDrawer() {
  state.currentSiteDetail = null;
  renderSiteDetailDrawer();
  activateView("sites", "sites-status");
}

function hydrateDnsProviderForm(provider) {
  const form = $("dns-provider-form");
  $("dns-provider-id-hidden").value = provider.provider_id || "";
  form.name.value = provider.name || "";
  form.provider_type.value = provider.provider_type || "cloudflare";
  form.api_endpoint.value = provider.api_endpoint || "";
  form.credentials.value = '{"api_token":"replace-with-token"}';
  setResultBox("dns-provider-form-result", {
    message: "请补全或替换凭据后重新保存 Provider",
    provider,
  });
  toast("Provider 已加载到表单", "success");
}

function hydrateOperationTemplateForm(template) {
  const form = $("operation-template-form");
  $("operation-template-id-hidden").value = template.template_id || "";
  form.name.value = template.name || "";
  form.operation_type.value = template.operation_type || "custom_template";
  form.command_template.value = template.command_template || "";
  form.allowed_params.value = (template.allowed_params || []).join(",");
  form.timeout_seconds.value = template.timeout_seconds || 60;
  form.run_as_user.value = template.run_as_user || "root";
  form.approval_required.checked = Boolean(template.approval_required);
  setResultBox("operation-template-form-result", {
    message: "请补全命令模板后重新保存",
    template,
  });
  toast("动作模板已加载到表单", "success");
}

async function loadSiteIntoForm(siteId, options = {}) {
  try {
    const site = await apiRequest(`/api/admin/sites/${siteId}`);
    $("site-id").value = site.site_id;
    $("site-form-title").textContent = `编辑站点 · ${site.name}`;
    $("site-submit-button").textContent = "保存修改";
    const form = $("site-form");
    form.name.value = site.name;
    form.domain.value = site.domain;
    form.listen_port.value = site.listen_port;
    form.protocol.value = site.protocol;
    form.tls_enabled.checked = site.tls_enabled;
    applyManagedUpstreamsToForm(form, site.config || {});
    applyManagedCacheRulesToForm(form, site.config || {});
    form.config.value = formatJson(mergeManagedSiteConfig(site.config || {}, form));

    $("binding-site-id").value = site.site_id;
    $("binding-rows").innerHTML = "";
    const bindings = site.bindings || [];
    if (bindings.length === 0) {
      addBindingRow();
    } else {
      bindings.forEach((binding) => addBindingRow(binding));
    }

    activateView("sites", options.subsection || "sites-config");
    window.scrollTo({ top: 0, behavior: "smooth" });
    if (options.toastMessage !== false) {
      toast("站点详情已加载到页面", "success");
    }
  } catch (error) {
    setResultBox("site-form-result", { error: error.message });
    toast(error.message || "加载站点详情失败", "error");
  }
}

function resetSiteForm() {
  const form = $("site-form");
  form.reset();
  $("site-id").value = "";
  $("site-form-title").textContent = "创建站点";
  $("site-submit-button").textContent = "创建站点";
  form.protocol.value = "https";
  form.listen_port.value = 443;
  form.tls_enabled.checked = true;
  applyManagedUpstreamDefaults(form);
  applyManagedCacheDefaults(form);
  form.config.value = formatJson(defaultSiteConfigObject());
  $("upstream-new-name").value = "";
}

function setResultBox(id, payload) {
  $(id).textContent = JSON.stringify(payload, null, 2);
}

function throwFormError(resultId, message) {
  setResultBox(resultId, { error: message });
  toast(message, "error");
}

async function apiRequest(path, options = {}) {
  const headers = {
    Accept: "application/json",
  };
  const request = {
    method: options.method || "GET",
    headers,
  };
  if (options.body !== undefined && options.body !== null) {
    headers["Content-Type"] = "application/json";
    request.body = JSON.stringify(options.body);
  }

  const response = await fetch(path, request);
  let payload = null;
  try {
    payload = await response.json();
  } catch (_error) {
    payload = null;
  }

  if (!response.ok) {
    if (response.status === 401) {
      window.location.assign("/login");
      throw new Error("登录状态已失效，请重新登录");
    }
    throw new Error(
      payload?.error?.message ||
        payload?.message ||
        `请求失败：${response.status} ${response.statusText}`,
    );
  }

  return payload?.data;
}

function renderStatusBadge(status) {
  const tone = statusTone(status);
  return `<span class="status-badge ${tone}">${escapeHtml(status || "-")}</span>`;
}

function renderReleaseCounts(counts) {
  return `
    <div class="pill-list">
      <span class="pill">pending ${escapeHtml(String(counts.pending))}</span>
      <span class="pill">in_progress ${escapeHtml(String(counts.in_progress))}</span>
      <span class="pill">success ${escapeHtml(String(counts.success))}</span>
      <span class="pill">failed ${escapeHtml(String(counts.failed))}</span>
    </div>
  `;
}

function renderEmptyRow(message, colspan) {
  return `<tr><td colspan="${colspan}"><div class="empty-state">${escapeHtml(
    message,
  )}</div></td></tr>`;
}

function certificateOrderSubject(order, site) {
  const identifier = order.challenge_payload?.identifier || order.challenge_payload?.record_name;
  if (site) {
    return `${site.name || site.site_code} · ${identifier || site.domain}`;
  }
  if (identifier) {
    return `${identifier} · 独立证书`;
  }
  return order.site_id || "独立证书";
}

function detailItem(label, value, fullSpan = false) {
  const tagName = String(value || "").includes("\n") ? "code" : "strong";
  return `
    <div class="detail-item ${fullSpan ? "full-span" : ""}">
      <span>${escapeHtml(label)}</span>
      <${tagName}>${escapeHtml(value || "-")}</${tagName}>
    </div>
  `;
}

function buildNodeIpText(detail) {
  const segments = [];
  if (detail.private_ip) {
    segments.push(`内网 ${detail.private_ip}`);
  }
  if (detail.public_ip) {
    segments.push(`公网 ${detail.public_ip}`);
  }
  return segments.join(" / ") || "-";
}

function closeActionMenu(element) {
  const menu = element?.closest(".action-menu");
  if (menu) {
    menu.open = false;
  }
}

function compareNodeMonitorRows(left, right) {
  const leftLag = left.lagSeconds ?? Number.MAX_SAFE_INTEGER;
  const rightLag = right.lagSeconds ?? Number.MAX_SAFE_INTEGER;
  return leftLag - rightLag;
}

function computeLagSeconds(value) {
  if (!value) {
    return null;
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return null;
  }
  return Math.max(0, Math.round((Date.now() - date.getTime()) / 1000));
}

function formatLag(seconds) {
  if (seconds === null || seconds === undefined) {
    return "未上报";
  }
  if (seconds < 60) {
    return `${seconds}s`;
  }
  if (seconds < 3600) {
    return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
  }
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

function summarizeNodeOperations(operations) {
  return (Array.isArray(operations) ? operations : []).reduce(
    (summary, operation) => {
      const key = operation.exec_status || "pending";
      summary[key] = (summary[key] || 0) + 1;
      return summary;
    },
    {
      pending: 0,
      approved: 0,
      running: 0,
      success: 0,
      failed: 0,
      timeout: 0,
      cancelled: 0,
    },
  );
}

function summarizeOperationOutput(operation) {
  const candidates = [
    operation.stdout_log,
    operation.stderr_log,
    operation.exit_code !== null && operation.exit_code !== undefined
      ? `exit_code=${operation.exit_code}`
      : "",
  ]
    .map((item) => String(item || "").trim())
    .filter(Boolean);
  if (candidates.length === 0) {
    return "暂无回传输出";
  }
  return candidates.join(" | ").slice(0, 180);
}

function statusTone(status) {
  const value = (status || "").toLowerCase();
  if (
    [
      "online",
      "success",
      "published",
      "active",
      "issued",
      "available",
    ].includes(value)
  ) {
    return "status-success";
  }
  if (
    [
      "pending",
      "draft",
      "staging",
      "suspect",
      "approved",
      "in_progress",
      "publishing",
      "applying",
      "downloading",
      "running",
      "pending_dns_challenge",
      "dns_challenge_presented",
      "dns_challenge_presenting",
    ].includes(value)
  ) {
    return "status-warning";
  }
  if (
    [
      "failed",
      "offline",
      "dns_challenge_failed",
      "issue_failed",
      "maintenance",
      "timeout",
      "cancelled",
    ].includes(value)
  ) {
    return "status-danger";
  }
  return "status-neutral";
}

function statusAccent(status) {
  const tone = statusTone(status);
  if (tone === "status-success") {
    return "accent-green";
  }
  if (tone === "status-danger") {
    return "accent-rose";
  }
  return "accent-amber";
}

function parseCommaList(value) {
  return [...new Set(String(value || "")
    .split(/[\n,]+/)
    .map((item) => item.trim())
    .filter(Boolean))];
}

function parseJsonInput(text, fallbackValue) {
  const trimmed = text.trim();
  if (!trimmed) {
    return fallbackValue;
  }
  return JSON.parse(trimmed);
}

function syncSiteConfigPreview() {
  const form = $("site-form");
  try {
    const baseConfig = parseJsonInput(form.config.value, {});
    form.config.value = formatJson(mergeManagedSiteConfig(baseConfig, form));
  } catch (_error) {
    // Skip preview sync when the JSON editor is temporarily invalid.
  }
}

function mergeManagedSiteConfig(baseConfig, form) {
  const config = { ...baseConfig };
  const upstreams = collectManagedUpstreams(form);
  config.upstreams = upstreams;

  const existingRules = Array.isArray(baseConfig?.cache_rules) ? baseConfig.cache_rules : [];
  const customRules = existingRules.filter(
    (rule) => !MANAGED_CACHE_RULES.some((managedRule) => managedRule.ruleName === rule?.name),
  );
  const managedRules = MANAGED_CACHE_RULES.map((rule) => buildManagedCacheRule(form, rule)).filter(
    Boolean,
  );
  const cacheRules = [...customRules, ...managedRules];
  if (cacheRules.length > 0) {
    config.cache_rules = cacheRules;
  } else {
    delete config.cache_rules;
  }
  return config;
}

function collectManagedUpstreams(form) {
  return [...form.querySelectorAll(".upstream-group")]
    .map((group) => {
      const name = group.querySelector('[data-upstream-field="name"]').value.trim();
      const endpoints = [...group.querySelectorAll(".upstream-endpoint-row")]
        .map((row) => ({
          address: row.querySelector('[data-endpoint-field="address"]').value.trim(),
          weight: Number(row.querySelector('[data-endpoint-field="weight"]').value || 100),
          active: row.querySelector('[data-endpoint-field="active"]').checked,
          backup: row.querySelector('[data-endpoint-field="backup"]').checked,
        }))
        .filter((endpoint) => endpoint.address)
        .map((endpoint) => ({
          address: endpoint.address,
          weight: Number.isFinite(endpoint.weight) ? Math.max(1, Math.round(endpoint.weight)) : 100,
          active: endpoint.active,
          backup: endpoint.backup,
        }));

      if (!name || endpoints.length === 0) {
        return null;
      }

      return { name, endpoints };
    })
    .filter(Boolean);
}

function applyManagedUpstreamsToForm(form, config) {
  const upstreamGroups = $("upstream-groups");
  upstreamGroups.innerHTML = "";
  const upstreams = Array.isArray(config?.upstreams) ? config.upstreams : [];
  if (upstreams.length === 0) {
    addUpstreamGroup(DEFAULT_UPSTREAM_TEMPLATE);
    return;
  }
  for (const upstream of upstreams) {
    addUpstreamGroup({
      name: upstream?.name || DEFAULT_UPSTREAM_TEMPLATE.name,
      endpoints: normalizeEndpointList(upstream?.endpoints),
    });
  }
}

function applyManagedUpstreamDefaults(form) {
  applyManagedUpstreamsToForm(form, {
    upstreams: [],
  });
}

function addUpstreamGroup(initial = DEFAULT_UPSTREAM_TEMPLATE) {
  const composer = $("upstream-new-name");
  const group = document.createElement("div");
  group.className = "upstream-group";
  group.innerHTML = `
    <div class="upstream-group-header">
      <label>
        <span>Upstream 名称</span>
        <input data-upstream-field="name" placeholder="origin-service" />
      </label>
      <div class="stack-actions">
        <button class="ghost-button" type="button" data-add-endpoint>添加地址</button>
        <button class="ghost-button" type="button" data-remove-upstream>移除 Upstream</button>
      </div>
    </div>
    <div class="upstream-endpoints"></div>
  `;
  $("upstream-groups").appendChild(group);
  group.querySelector('[data-upstream-field="name"]').value = initial.name || "";
  if (composer) {
    composer.value = "";
  }
  const endpoints = normalizeEndpointList(initial.endpoints);
  if (endpoints.length === 0) {
    addUpstreamEndpointRow(group);
  } else {
    endpoints.forEach((endpoint) => addUpstreamEndpointRow(group, endpoint));
  }
}

function addUpstreamEndpointRow(group, initial = {}) {
  const row = document.createElement("div");
  row.className = "upstream-endpoint-row";
  row.innerHTML = `
    <label>
      <span>地址</span>
      <input data-endpoint-field="address" placeholder="源站地址，如 10.0.0.10:8080" />
    </label>
    <label>
      <span>权重</span>
      <input data-endpoint-field="weight" type="number" min="1" step="1" value="100" />
    </label>
    <label class="checkbox-field toggle-field">
      <input data-endpoint-field="active" type="checkbox" checked />
      <span>启用</span>
    </label>
    <label class="checkbox-field toggle-field">
      <input data-endpoint-field="backup" type="checkbox" />
      <span>Backup</span>
    </label>
    <button class="ghost-button endpoint-remove-button" type="button" data-remove-endpoint>
      移除
    </button>
  `;
  group.querySelector(".upstream-endpoints").appendChild(row);
  row.querySelector('[data-endpoint-field="address"]').value = initial.address || "";
  row.querySelector('[data-endpoint-field="weight"]').value = initial.weight ?? 100;
  row.querySelector('[data-endpoint-field="active"]').checked = initial.active !== false;
  row.querySelector('[data-endpoint-field="backup"]').checked = initial.backup === true;
}

function handleUpstreamGroupClick(event) {
  const addEndpointButton = event.target.closest("[data-add-endpoint]");
  if (addEndpointButton) {
    addUpstreamEndpointRow(addEndpointButton.closest(".upstream-group"));
    return;
  }

  const removeEndpointButton = event.target.closest("[data-remove-endpoint]");
  if (removeEndpointButton) {
    const group = removeEndpointButton.closest(".upstream-group");
    const endpoints = group.querySelectorAll(".upstream-endpoint-row");
    if (endpoints.length <= 1) {
      endpoints[0]?.querySelector('[data-endpoint-field="address"]').focus();
      return;
    }
    removeEndpointButton.closest(".upstream-endpoint-row")?.remove();
    syncSiteConfigPreview();
    return;
  }

  const removeGroupButton = event.target.closest("[data-remove-upstream]");
  if (removeGroupButton) {
    removeGroupButton.closest(".upstream-group")?.remove();
    if ($("upstream-groups").children.length === 0) {
      addUpstreamGroup();
    }
    syncSiteConfigPreview();
  }
}

function normalizeEndpointList(endpoints) {
  return (Array.isArray(endpoints) ? endpoints : [])
    .map((endpoint) => {
      if (typeof endpoint === "string") {
        return { address: endpoint, weight: 100, active: true, backup: false };
      }
      if (!endpoint || typeof endpoint !== "object") {
        return null;
      }
      return {
        address: String(endpoint.address || "").trim(),
        weight: Number(endpoint.weight ?? 100),
        active: endpoint.active !== false,
        backup: endpoint.backup === true,
      };
    })
    .filter((endpoint) => endpoint && endpoint.address);
}

function buildManagedCacheRule(form, ruleConfig) {
  if (!form[ruleConfig.enabledField].checked) {
    return null;
  }
  const ttlValue = Number(form[ruleConfig.ttlField].value || 0);
  const matchExtensions = normalizeExtensionList(form[ruleConfig.extensionsField].value);
  if (!Number.isFinite(ttlValue) || ttlValue <= 0 || matchExtensions.length === 0) {
    return null;
  }
  const cacheControl =
    form[ruleConfig.cacheControlField].value.trim() ||
    `public, max-age=${ttlValue}, immutable`;
  return {
    name: ruleConfig.ruleName,
    match_extensions: matchExtensions,
    expires_seconds: Math.round(ttlValue),
    cache_control: cacheControl,
  };
}

function applyManagedCacheRulesToForm(form, config) {
  const rules = Array.isArray(config?.cache_rules) ? config.cache_rules : [];
  for (const ruleConfig of MANAGED_CACHE_RULES) {
    const matchedRule = rules.find((rule) => rule?.name === ruleConfig.ruleName);
    form[ruleConfig.enabledField].checked = Boolean(matchedRule);
    form[ruleConfig.extensionsField].value = matchedRule
      ? joinExtensionList(matchedRule.match_extensions)
      : ruleConfig.defaultExtensions;
    form[ruleConfig.ttlField].value = matchedRule?.expires_seconds ?? ruleConfig.defaultTtl;
    form[ruleConfig.cacheControlField].value =
      matchedRule?.cache_control ?? ruleConfig.defaultCacheControl;
  }
}

function applyManagedCacheDefaults(form) {
  for (const rule of MANAGED_CACHE_RULES) {
    form[rule.enabledField].checked = true;
  }
  applyManagedCacheRulesToForm(form, {
    cache_rules: MANAGED_CACHE_RULES.map((rule) => ({
      name: rule.ruleName,
      match_extensions: normalizeExtensionList(rule.defaultExtensions),
      expires_seconds: rule.defaultTtl,
      cache_control: rule.defaultCacheControl,
    })),
  });
}

function defaultSiteConfigObject() {
  return {
    upstreams: [],
    cache_rules: MANAGED_CACHE_RULES.map((rule) => ({
      name: rule.ruleName,
      match_extensions: normalizeExtensionList(rule.defaultExtensions),
      expires_seconds: rule.defaultTtl,
      cache_control: rule.defaultCacheControl,
    })),
  };
}

function normalizeExtensionList(value) {
  return [...new Set(String(value || "")
    .split(/[\n,]+/)
    .map((item) => item.trim().replace(/^\./, "").toLowerCase())
    .filter(Boolean))];
}

function joinExtensionList(values) {
  return (Array.isArray(values) ? values : []).join(",");
}

function formatJson(value) {
  return JSON.stringify(value, null, 2);
}

function isSameLocalDay(value, reference = new Date()) {
  if (!value) {
    return false;
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return false;
  }
  return (
    date.getFullYear() === reference.getFullYear() &&
    date.getMonth() === reference.getMonth() &&
    date.getDate() === reference.getDate()
  );
}

function formatDate(value) {
  if (!value) {
    return "-";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return String(value);
  }
  return new Intl.DateTimeFormat("zh-CN", {
    dateStyle: "short",
    timeStyle: "medium",
  }).format(date);
}

function setConnectionStatus(kind, text) {
  const pill = document.querySelector(".status-pill");
  if (!pill || !$("connection-status")) {
    return;
  }
  pill.classList.remove("connected", "error");
  if (kind === "connected") {
    pill.classList.add("connected");
  } else if (kind === "error") {
    pill.classList.add("error");
  }
  $("connection-status").textContent = text;
}

function toast(message, kind = "success") {
  const item = document.createElement("div");
  item.className = `toast ${kind}`;
  item.textContent = message;
  $("toast-stack").appendChild(item);
  window.setTimeout(() => {
    item.remove();
  }, 3200);
}

function safeToast(message, kind = "success") {
  try {
    if ($("toast-stack")) {
      toast(message, kind);
    }
  } catch (_error) {
    // Keep initialization failures visible even if the toast container is missing.
  }
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
