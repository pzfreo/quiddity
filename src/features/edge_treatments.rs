//! Structural edge treatments used to measure their supporting features (#134).
//! Reuse the treatment proofs; a merely sloping or curved neighbour is not an edge break.

use super::evidence::Occurrence;
use super::{Context, chamfers, fillets, turned};
use crate::kernel::geom::{self, Surface};

pub(super) fn discover(ctx: &Context<'_>) -> Vec<Occurrence<()>> {
    let erase = |defining, context| Occurrence {
        record: (),
        defining,
        context,
    };
    let mut out: Vec<_> = ctx
        .reported_chamfers()
        .iter()
        .map(|o| erase(o.defining.clone(), o.context.clone()))
        .chain(
            ctx.reported_oriented_chamfers()
                .iter()
                .map(|o| erase(o.defining.clone(), o.context.clone())),
        )
        .chain(
            ctx.reported_fillets()
                .iter()
                .filter(|o| o.record.side == "convex")
                .map(|o| erase(o.defining.clone(), o.context.clone())),
        )
        .collect();
    // Reuse reported proofs. Only sub-reporting-radius cylinders need a new
    // prismatic proof; a small real round still trims its supporting skin.
    let max_radius = fillets::FilletOptions::default().max_radius_frac * ctx.bounds().max_extent();
    for (face, f) in ctx.part.faces.iter().enumerate() {
        if matches!(f.surface, Surface::Cylinder { radius, .. } if radius < fillets::MIN_RADIUS)
            && let Some(o) = fillets::prismatic(ctx, face, 0., max_radius)
            && o.record.side == "convex"
        {
            out.push(erase(o.defining, o.context));
        }
    }
    // Structural measurements must also work on an oblique boss. The standalone
    // turned-treatment records use principal-axis presentation; apply those same
    // geometric proofs here without that presentation gate (#134).
    let external = turned::external_cylinders(ctx);
    let internal = fillets::internal_cylinders(ctx);
    let max_leg = chamfers::ChamferOptions::default().max_leg_frac * ctx.bounds().max_extent();
    let max_radius = fillets::FilletOptions::default().max_radius_frac * ctx.bounds().max_extent();
    for (face, f) in ctx.part.faces.iter().enumerate() {
        if out.iter().any(|o| o.defining.contains(&face)) {
            continue;
        }
        let context = match f.surface {
            Surface::Cone { frame, .. } => {
                let Some(((r0, c0), (r1, c1), _)) = turned::cone_rims(ctx.part, face) else {
                    continue;
                };
                if (r1 - r0).max(geom::dot(geom::sub(c1, c0), frame.z).abs()) > max_leg {
                    continue;
                }
                let neighbours = ctx.part.neighbours(face);
                let coaxial =
                    turned::coaxial_cylinders(&neighbours, &external, frame.origin, frame.z);
                if coaxial.is_empty()
                    || (turned::transverse_planes(ctx.part, &neighbours, frame.z).is_empty()
                        && !turned::bridges_two_bands(&coaxial))
                {
                    continue;
                }
                vec![]
            }
            Surface::Torus { minor, .. }
                if minor <= max_radius && fillets::torus_side(ctx.part, face) == Some("convex") =>
            {
                let Some((_, _, lo, hi)) = ctx.part.uv_bounds(face) else {
                    continue;
                };
                if (hi - lo).abs() > fillets::TURNED_FILLET_MAX_EXTENT {
                    continue;
                }
                let neighbours = ctx.part.neighbours(face);
                let Some(context) = fillets::turned_context(ctx.part, face, &neighbours, &external)
                    .or_else(|| fillets::slot_end_context(ctx.part, face, &neighbours, &internal))
                else {
                    continue;
                };
                context
            }
            _ => continue,
        };
        out.push(erase(vec![face], context));
    }
    out
}
