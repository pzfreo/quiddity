//! Decide recognition's boundary in the source frame. The distance belongs to
//! the STEP edge, while the threshold belongs to each incident face's context.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BoundaryPolicy {
    #[default]
    Master,
    Recognition,
    Projected,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EdgeDeviation {
    work: u64,
    maximum: Option<f64>,
}

#[cfg(test)]
impl EdgeDeviation {
    pub(super) fn work_for_test(&self) -> u64 {
        self.work
    }
}

pub(crate) type EdgeDeviations = Arc<[OnceLock<EdgeDeviation>]>;

impl Part {
    pub(super) fn declares_disagreeing_edge(&self, face: usize) -> Option<bool> {
        let Some(&tolerance) = self.measure_uncertainty.get(&face) else {
            return Some(false);
        };
        let mut disagrees = false;
        // Seams have two coedges but only one STEP master. A budget charges
        // the logical cost of each distinct incident edge, including cached
        // checks, so a face's verdict cannot depend on query or face order.
        for edge in self.face_edges(face) {
            if self.measure_trims.get(&edge).is_some_and(Option::is_some) {
                disagrees |= self
                    .edge_master_deviation(edge)?
                    .is_some_and(|d| d > tolerance);
            }
        }
        Some(disagrees)
    }

    fn edge_master_deviation(&self, edge: usize) -> Option<Option<f64>> {
        let entries = self
            .master_deviations
            .get_or_init(|| (0..self.edges.len()).map(|_| OnceLock::new()).collect());
        let entry = &entries[edge];
        if !work_meter_active() {
            // Outside a budget, the check cannot abort on work. Cache successful
            // comparisons and unresolvable trims alike.
            return Some(
                entry
                    .get_or_init(|| self.measure_master_deviation(edge))
                    .maximum,
            );
        }
        if let Some(cached) = entry.get() {
            spend_work_units(cached.work)?;
            return Some(cached.maximum);
        }
        let measured = self.measure_master_deviation(edge);
        if work_exhausted() {
            return None;
        }
        let _ = entry.set(measured);
        Some(measured.maximum)
    }

    fn measure_master_deviation(&self, e: usize) -> EdgeDeviation {
        let work = std::cell::Cell::new(0);
        let spend = || {
            spend_work()?;
            work.set(work.get() + 1);
            Some(())
        };
        let maximum = (|| {
            let (carrier, raw) = self.measure_trims.get(&e)?.as_ref()?;
            if self.unresolved_edges().binary_search(&e).is_ok() {
                return None;
            }
            let units = self.file_units()?;
            let limit = self.edge_faces()[e]
                .iter()
                .filter_map(|f| self.measure_uncertainty.get(f).copied())
                .reduce(f64::min)?;
            let detached;
            let (surface, owner) = match carrier {
                TrimCarrier::Faces(owners) => {
                    let face = *owners.first()?;
                    (&self.faces[face].surface, Some(face))
                }
                TrimCarrier::Detached(s) => {
                    detached = s.placed(&self.edge_placement(e));
                    (&detached, None)
                }
            };
            let (scales, axes) = match surface.file_parameter_scales(units) {
                Some(scales) => (scales, [Axis::ID; 2]),
                None => ((1., 1.), parameters::swept_axes(self, owner?)?),
            };
            let curve = ParamCurve {
                source: scaled_pcurve(raw, scales.0, scales.1)?,
                axes,
                transfer: None,
            };
            let edge = &self.edges[e];
            let spatial = edge_interval(
                &edge.curve,
                edge.start,
                edge.end,
                edge.same_sense,
                edge.is_closed(),
            );
            let interval = resolve_interval(
                surface,
                edge,
                &curve,
                spatial,
                limit + geom::COORD_FLOOR,
                spend,
            )?;
            let trim = EdgeTrim {
                curve,
                spans: closed_spans(surface),
                interval,
                samples: OnceLock::new(),
                mesh_samples: OnceLock::new(),
                nodes: OnceLock::new(),
                length: OnceLock::new(),
            };
            max_deviation(surface, &trim, edge, spatial, spend)
        })();
        EdgeDeviation {
            work: work.get(),
            maximum,
        }
    }
}

fn max_deviation(
    surface: &Surface,
    trim: &EdgeTrim,
    edge: &crate::brep::Edge,
    (lo, hi): (f64, f64),
    spend: impl Fn() -> Option<()>,
) -> Option<f64> {
    let (a, b) = trim.interval;
    let (lo, hi) = (lo.min(hi), lo.max(hi));
    if !(lo.is_finite() && hi.is_finite() && hi > lo) {
        return None;
    }
    let spatial = &edge.curve;
    let period = spatial.period();
    let ends = [spatial.value(lo), spatial.value(hi)];
    let spline_spans = if let Curve::Nurbs(c) = spatial {
        let (start, stop) = c.domain();
        let from = period.map_or(lo, |p| start + (lo - start).rem_euclid(p));
        let to = (from + (hi - lo)).min(stop);
        let wrap = (from + (hi - lo) - stop).max(0.);
        Some((
            c,
            (from, to, [c.value(from), c.value(to)]),
            (wrap > 0.).then(|| (start, start + wrap, [c.value(start), c.value(start + wrap)])),
        ))
    } else {
        None
    };
    let nearest = |p| {
        if let Some((c, (a, b, ends), wrap)) = spline_spans {
            let first = c.trimmed_distance(p, a, b, ends);
            return wrap.map_or(first, |(a, b, ends)| {
                first.min(c.trimmed_distance(p, a, b, ends))
            });
        }
        let mut t = spatial.parameter(p);
        if let Some(period) = period {
            // Use the turn containing this trim, choosing the nearer endpoint if the
            // foot lies on its omitted arc.
            t += period * ((0.5 * (lo + hi) - t) / period).round();
        }
        geom::dist(p, spatial.value(t.clamp(lo, hi)))
            .min(geom::dist(p, ends[0]))
            .min(geom::dist(p, ends[1]))
    };
    let (carrier, spans) = trim
        .curve
        .transfer
        .as_ref()
        .map_or((surface, trim.spans), |tr| (&*tr.carrier, tr.carrier_spans));
    let cuts = trim_spans(trim)?;
    let mut maximum: f64 = 0.;
    // Thirty-two subdivisions per pcurve knot span resolve local spline excursions;
    // at least 128 over the full trim also resolve bends induced by the carrier along
    // long analytic spans (about three degrees apart on a complete revolution).
    for (i, w) in cuts.windows(2).enumerate() {
        let n = (128. * ((w[1] - w[0]) / (b - a)).abs()).ceil().max(32.) as usize;
        for j in usize::from(i != 0)..=n {
            let t = if j == n {
                w[1]
            } else {
                w[0] + (w[1] - w[0]) * j as f64 / n as f64
            };
            let (u, v) = carrier_uv(carrier, spans, trim.curve.carrier_uv(t), &spend)?;
            spend()?;
            let xyz = carrier.value(u, v);
            if !xyz.iter().all(|x| x.is_finite()) {
                return None;
            }
            let distance = nearest(xyz);
            if !distance.is_finite() {
                return None;
            }
            maximum = maximum.max(distance);
        }
    }
    Some(maximum)
}

fn trim_spans(trim: &EdgeTrim) -> Option<Vec<f64>> {
    let (a, b) = trim.interval;
    let knots = match &trim.curve.source {
        Curve::Nurbs(c) => knots_on_interval(c.breaks(), trim.curve.source.period(), a, b)?,
        _ => Vec::new(),
    };
    cuts(a, b, 1, &knots)
}
