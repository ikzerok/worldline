use super::*;

impl Project {
    pub fn apply_reconciliation(
        &mut self,
        plan: &ReconciliationPlan,
    ) -> Result<ReconciliationApplied, String> {
        self.apply_reconciliation_with_progress(plan, &mut |_| true)
    }

    pub fn apply_reconciliation_with_progress(
        &mut self,
        plan: &ReconciliationPlan,
        progress: Progress<'_>,
    ) -> Result<ReconciliationApplied, String> {
        let prepared = self.prepare_reconciliation_with_progress(plan, progress)?;
        checkpoint(progress, ReconciliationStage::BeforeCommit)?;
        self.commit_prepared_reconciliation(prepared)
    }

    /// 后台只准备不可伪造候选；主线程提交时仍对真实Project和磁盘作最终完整核验。
    pub fn prepare_reconciliation_with_progress(
        &self,
        plan: &ReconciliationPlan,
        progress: Progress<'_>,
    ) -> Result<PreparedReconciliation, String> {
        capture::storage_ready(self)?;
        let verified =
            self.preview_reconciliation_with_progress(&plan.session, &plan.request, progress)?;
        if &verified != plan {
            return Err("外部改稿计划被修改或已经过期，工程未修改".into());
        }
        if !verified.can_apply {
            return Err(format!(
                "外部改稿尚不能采纳：{}",
                verified.blockers.join("；")
            ));
        }
        let (candidate, _, blockers) =
            candidate::build_candidate(self, &plan.session, &plan.request, progress)?;
        if !blockers.is_empty() {
            return Err(blockers.join("；"));
        }
        Ok(PreparedReconciliation {
            plan: verified,
            candidate,
        })
    }

    pub fn commit_prepared_reconciliation(
        &mut self,
        prepared: PreparedReconciliation,
    ) -> Result<ReconciliationApplied, String> {
        let PreparedReconciliation {
            plan,
            mut candidate,
        } = prepared;
        // 位于全部编译、报告、后台传输与可取消回调之后，不接受第二次外改。
        capture::storage_ready(self)?;
        for file in &plan.session.files {
            crate::source_lifecycle::safety::writable_path(&self.root.join(&file.path))?;
        }
        if capture::capture_guard(self, &mut |_| true)? != plan.session.guard {
            return Err("提交前磁盘或工程再次变化，整批未采纳；原候选已保留".into());
        }
        capture::storage_ready(self)?;
        let generation = self.refresh_generation.wrapping_add(1);
        let mut undo = self.clone();
        undo.refresh_generation = generation;
        candidate.refresh_generation = generation;
        // 专用undo只退作者正文；基线仍对应刚核验磁盘，绝不倒回第三方改稿之前。
        for file in &plan.session.files {
            let path = self.root.join(&file.path);
            if file.authoring {
                undo.authoring_documents
                    .get_mut(&path)
                    .ok_or("撤销文档身份消失")?
                    .saved = file.disk.clone();
            } else {
                undo.documents
                    .get_mut(&path)
                    .ok_or("撤销源码身份消失")?
                    .saved = file
                    .disk
                    .as_ref()
                    .map(|bytes| String::from_utf8(bytes.clone()))
                    .transpose()
                    .map_err(|_| "撤销基线不是UTF-8")?;
            }
        }
        // 磁盘清单新登记的已有文件不是本地新建。旧撤销若没有这些缓冲，
        // 通用restore会误加删除墓碑；专用undo须把外部原稿作为干净缓冲保留。
        for (path, document) in &candidate.authoring_documents {
            if !undo.authoring_documents.contains_key(path) {
                if document.is_dirty()
                    || document.saved.as_ref() != plan.session.guard.disk.get(path)
                {
                    return Err("新增登记文件缺少准确磁盘基线，无法安全建立撤销".into());
                }
                undo.authoring_documents
                    .insert(path.clone(), document.clone());
            }
        }
        *self = candidate;
        Ok(ReconciliationApplied { plan, undo })
    }
}
