# Reuse exact NURBS operands within a face integral

Repeated surface evaluations in a face integral rebuild the same parameter basis and homogeneous control block. I retain the last exact clamped parameter on each axis and the current span pair's control block. The original u/v/component accumulation order, quadrature nodes, convergence criteria and refusal conditions are unchanged.

The directed regressions compare the affected computations with their original
paths, including refusal and boundary cases. They do not replace full workspace
regression or strict snapshot reproduction on the standard ARM macOS runner.
No expected snapshots, reference bands or inventory floors are changed.
