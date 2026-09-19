//! Viewport synchronization and PTY resize lifetime.

use super::*;

impl TerminalView {
    /// 宿主宣告：下一次网格变化来自结构性布局改动，直接提交、不去抖。
    /// 见 `structural_resize` 字段注释。
    pub fn mark_structural_resize(&mut self) {
        self.structural_resize = true;
    }

    /// 元素 prepaint 回写布局：内容矩形与度量交给渲染合同裁定网格。
    /// 网格变化时同步 Term 与 ConPTY；行列不变但像素口径变化也上报 PTY
    /// （应用可能关心像素度量）；稳态帧 observe 返回 None，零额外开销。
    pub fn set_layout(
        &mut self,
        origin: Point<Pixels>,
        cell_width: Pixels,
        line_height: Pixels,
        content: Size<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        self.origin = origin;
        self.cell_width = cell_width;
        self.line_height = line_height;
        let metrics = CellMetrics {
            cell_width: cell_width.as_f32(),
            cell_height: line_height.as_f32(),
            scale,
        };
        let change =
            self.viewports.observe(content.width.as_f32(), content.height.as_f32(), &metrics);

        // 启动稳定闸：开窗 resize 异步落地，首帧可能还是开窗前的旧尺寸。
        // 命中 spawn 网格前不向 Term/ConPTY 下发（PTY 出生即目标几何，
        // 零下发收口）；宽限期后仍未命中（小屏收拢等真实差异）则放行，
        // 一次性按当前视口纠正。observe 会把过渡帧并进 current，因此
        // 释放判定不依赖本帧是否有增量。
        if !self.grid_synced {
            let Some(viewport) =
                change.map(|c| c.viewport).or_else(|| self.viewports.current().copied())
            else {
                return;
            };
            let landed = (viewport.cols, viewport.rows)
                == (self.window_size.num_cols, self.window_size.num_lines);
            if !landed && self.spawn_at.elapsed() < Self::STARTUP_GRID_GRACE {
                // A quiet shell may never produce another frame after this one.
                // Arrange an owned deadline, keeping the latest observed geometry.
                let first = self.pending_resize.replace(viewport).is_none();
                if first {
                    self.schedule_startup_grid_sync(cx);
                }
                return;
            }
            self.pending_resize = None;
            self.grid_synced = true;
            self.cols = viewport.cols as usize;
            self.rows = viewport.rows as usize;
            if !landed {
                self.commit_viewport(viewport);
            } else {
                self.window_size = viewport.window_size();
            }
            return;
        }

        let Some(change) = change else {
            return;
        };
        let viewport = change.viewport;
        self.cols = viewport.cols as usize;
        self.rows = viewport.rows as usize;

        // 本地网格立刻跟手，只有子进程那一半去抖（旧壳也通过
        // `resize_active_layout_grids` 采用同一策略）。两半的代价完全不对称：客户端
        // reflow 便宜且可逆，而每一次 `ResizePseudoConsole` 都让 conhost 重排
        // 自己的缓冲区，那些重排累积出的光标行漂移事后无从察觉。让网格落后于
        // 渲染就只能靠"视觉裁剪"预览未提交的几何，而裁剪只能裁行、无法重排列
        // ——宽度一变预览就是错的，且 Term 与屏幕不一致的每一毫秒里到达的字节
        // 都会按旧宽度进网格。
        if change.grid_changed {
            self.resize_grid_only(viewport);
        }

        // 结构性变化（分屏创建/关闭、zoom、面板开合）不去抖：它只来一次，没有
        // 后续帧可以合并，多等的每一毫秒都是子进程按旧几何输出的窗口期。旧壳在
        // 这些路径上走 `resize_active_layout()` 同步下发，这里复刻同一条合同。
        if std::mem::take(&mut self.structural_resize) {
            self.pending_resize = None;
            self.resize_epoch = self.resize_epoch.wrapping_add(1);
            self.commit_viewport(viewport);
            return;
        }

        // ConPTY 的直通式 conhost 在 resize 时零输出，指望终端侧 reflow 与
        // 它内部 buffer rewrap 一致；两者的换行语义存在路径依赖差异，每多
        // 一次中间宽度的 ResizePseudoConsole 就多攒一分光标行漂移（字节取
        // 证：13 次提交后 PSReadLine 的 CUP 行比真实提示行高 7 行）。旧壳
        // (winit) 的模态拖拽天然只在松手后送达一次 resize，从不累积。这里
        // 复刻该合同：子进程那一半纯尾沿去抖——只进 pending，视口静默
        // RESIZE_SETTLE_DELAY 后一次性下发。净零手势（挤压后拖回原宽）最终
        // 提交同尺寸 no-op，rewrap 次数为零；网格已在上面逐帧跟手，所以去抖
        // 的代价只落在"子进程晚知道几十毫秒"，屏幕上看不出来。
        self.pending_resize = Some(viewport);
        self.schedule_settled_resize(cx);
    }

    fn schedule_startup_grid_sync(&mut self, cx: &mut Context<Self>) {
        let delay = Self::STARTUP_GRID_GRACE.saturating_sub(self.spawn_at.elapsed());
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(delay).await;
            let _ = this.update(cx, |view, cx| {
                if view.grid_synced {
                    return;
                }
                let Some(viewport) = view.pending_resize.take() else { return };
                view.grid_synced = true;
                view.cols = viewport.cols as usize;
                view.rows = viewport.rows as usize;
                view.commit_viewport(viewport);
                cx.notify();
            });
        })
        .detach();
    }

    /// 只让本地网格 reflow 到 `viewport`，子进程留在旧几何上。
    ///
    /// 走 `Msg::ResizeGrid` 而不是直接锁 `Term`：event_loop 的 resize 分支会先
    /// 把旧几何下已可读的字节全部消化掉，绝对 CUP 序列因此不会被解析进新宽度
    /// 的网格。UI 线程自己上锁 resize 就绕过了这道流边界保护。
    fn resize_grid_only(&mut self, viewport: TerminalViewport) {
        let Some(session) = &self.session else { return };
        let mut notifier = nebula_terminal::event_loop::Notifier(session.notifier.0.clone());
        notifier.on_resize_grid(viewport.window_size());
    }

    /// Commit one viewport in grid-before-PTY order. Output produced after
    /// `ResizePseudoConsole` therefore always parses against the same geometry
    /// history ConPTY used to generate its absolute cursor coordinates.
    fn commit_viewport(&mut self, viewport: TerminalViewport) {
        let next = viewport.window_size();
        let grid_changed = (self.window_size.num_cols, self.window_size.num_lines)
            != (next.num_cols, next.num_lines);
        let pixel_changed = (self.window_size.cell_width, self.window_size.cell_height)
            != (next.cell_width, next.cell_height);
        if !grid_changed && !pixel_changed {
            return;
        }

        if let Some(session) = &self.session {
            let mut notifier = nebula_terminal::event_loop::Notifier(session.notifier.0.clone());
            notifier.on_resize(next);
        }
        self.window_size = next;
    }

    /// 一次拖拽手势（窗口边框/分屏把手）是否仍在进行。conhost 的 buffer
    /// rewrap 与本地 reflow 的换行语义存在路径依赖差异，每一次中间几何的
    /// `ResizePseudoConsole` 都会累积光标行漂移（字节取证：一次拖拽 14 次
    /// 提交后 PSReadLine 的 CUP 行比真实提示行高 7 行，且 conhost 全程零
    /// 重绘字节，漂移无法事后察觉）。旧壳 (winit) 的模态拖拽天然只在松手
    /// 后送达一次 resize，从不出这个问题；GPUI 在模态循环内持续派发布局，
    /// 时间去抖（150ms settle）与布局批次同周期，挡不住中间提交。因此按
    /// 手势门控：左键仍按住就不提交，settle 定时器自我续期到松手为止。
    #[cfg(windows)]
    fn drag_gesture_active() -> bool {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
        // SAFETY: GetAsyncKeyState 只读全局按键状态，无副作用。
        (unsafe { GetAsyncKeyState(VK_LBUTTON as i32) } as u16 & 0x8000) != 0
    }

    #[cfg(not(windows))]
    fn drag_gesture_active() -> bool {
        false
    }

    fn schedule_settled_resize(&mut self, cx: &mut Context<Self>) {
        self.resize_epoch = self.resize_epoch.wrapping_add(1);
        let epoch = self.resize_epoch;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(Self::RESIZE_SETTLE_DELAY).await;
            let _ = this.update(cx, |view, cx| {
                if view.resize_epoch != epoch {
                    return;
                }
                let gate = Self::drag_gesture_active();
                if std::env::var_os("NEBULA_RESIZE_TRACE").is_some() {
                    crate::gpui_shell::try_write_stderr(format_args!(
                        "[nebula:resize-trace] settle-timer gate={gate}"
                    ));
                }
                if gate {
                    // 手势未松开：净零手势（挤压后拖回原宽）最终提交同尺寸
                    // no-op，ConPTY 一次 rewrap 都不做。
                    view.schedule_settled_resize(cx);
                    return;
                }
                let Some(viewport) = view.pending_resize.take() else { return };
                view.commit_viewport(viewport);
                cx.notify();
            });
        })
        .detach();
    }
}
