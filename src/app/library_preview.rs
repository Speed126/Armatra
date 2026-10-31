//! Library activation is transactional; selection/query never enter this path.
use super::*;
fn preview_time(time: f32, duration: f32) -> Result<f32> {
    anyhow::ensure!(
        duration.is_finite()
            && duration >= 0.
            && time.is_finite()
            && time >= 0.
            && time <= duration + 1e-5,
        "Preview time is outside 0..{duration} seconds"
    );
    Ok(time.min(duration))
}
impl Editor {
    pub(super) fn preview_failed(&mut self, name: &str, reason: &str) {
        let message = format!("Preview failed · {name}: {reason}");
        crate::app_log::event(
            "preview_failed",
            serde_json::json!({"generation":self.clip_generation.0,"message":message}),
        );
        self.motion.pending_preview = None;
        self.motion.display_pending = None;
        self.motion.preview_error = Some(message.clone());
        self.status = message;
    }
    pub(super) fn preview_boundary(&self, boundary: &str) {
        if self.motion.display_pending.is_some() || self.motion.pending_preview.is_some() {
            crate::app_log::event(boundary, self.preview_identity());
        }
    }
    pub(super) fn preview_identity(&self) -> serde_json::Value {
        serde_json::json!({"record":self.actual_motion_id(),"source":self.source.as_ref().map(|s|&s.0),"clip":self.source.as_ref().map(|s|s.2),"generation":self.source_generation,"requested_generation":self.clip_generation.0,"target":self.model.as_ref().map(|m|&m.0),"source_skin":self.mapping_settings.source_skin,"target_skin":self.mapping_settings.target_skin,"active_plan":self.active_plan_id(),"time":self.playback.time,"playing":self.playback.playing,"resident":self.source_preview.as_ref().map(|p|&p.key),"pending_record":self.motion.pending_preview.as_ref().map(|p|&p.record_id)})
    }
    pub(super) fn preview_displayed(&mut self) {
        let Some((generation, id)) = self.motion.display_pending.take() else {
            return;
        };
        if self.source_generation != generation || self.actual_motion_id() != Some(id.as_str()) {
            return;
        }
        let mut data = self.preview_identity();
        data["frame_proof"] = self
            .motion
            .frame_proof
            .take()
            .unwrap_or(serde_json::Value::Null);
        crate::app_log::event("preview_first_displayed_frame", data.clone());
        self.motion.displayed_count += 1;
        if self.motion.displayed.len() == 256 {
            self.motion.displayed.remove(0);
        }
        self.motion.displayed.push(data);
    }
    pub(super) fn preflight_preview(
        &mut self,
        record: &Record,
        asset: &Arc<Asset>,
        time: f32,
        renderer: &Renderer,
        gpu: &Gpu,
    ) -> Result<()> {
        let Kind::Clip(index) = record.kind else {
            anyhow::bail!("Record is not a motion");
        };
        let clip = asset
            .clips
            .get(index)
            .context("Clip no longer exists in this asset")?;
        anyhow::ensure!(clip.supported, "Unsupported animation channels/deformation");
        let time = preview_time(time, clip.duration)?;
        let pose = animation::evaluate(
            asset,
            clip,
            &(0..asset.nodes.len()).map(Some).collect::<Vec<_>>(),
            time,
            &mut vec![],
        );
        anyhow::ensure!(
            asset.globals(&pose).iter().all(|m| m.is_finite()),
            "Motion has non-finite transforms at the requested time"
        );
        self.preview_boundary("preview_pose_validated");
        // Prepare potentially required fallback resources before replacing a
        // valid source/plan/playback. Failed bounded uploads leave it intact.
        if !self.gpu_cache.contains(&record.hash) {
            self.preview_boundary("preview_gpu_upload_begin");
            let resident = renderer.upload(gpu, asset)?;
            self.gpu_cache.pin(
                self.model
                    .iter()
                    .map(|m| m.0.clone())
                    .chain(self.source.iter().map(|s| s.0.clone()))
                    .chain([record.hash.clone()]),
            );
            self.gpu_cache
                .insert(record.hash.clone(), Arc::clone(&resident), resident.bytes);
            self.preview_boundary("preview_gpu_upload_complete");
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_accepts_endpoints_and_still_poses() {
        for duration in [0., 0.001, 1., 10000.] {
            for time in [0., duration / 2., duration] {
                assert_eq!(preview_time(time, duration).unwrap(), time);
            }
        }
    }
    #[test]
    fn preview_rejects_invalid_times_before_activation() {
        for time in [-1., f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.01] {
            assert!(preview_time(time, 1.).is_err());
        }
        for duration in [-1., f32::NAN, f32::INFINITY] {
            assert!(preview_time(0., duration).is_err());
        }
    }
}
