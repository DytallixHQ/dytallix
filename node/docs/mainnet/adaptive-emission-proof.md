# Adaptive emission: proof and limits

Status: engineering derivation for independent review. No calibrated mainnet
plant or approved gain table exists in this batch.

## 1. Arithmetic and state invariants

Assume a configuration accepted by `Config::validate` and a valid controller
state. Let S=10^6 and W<=65,536. Each error has absolute value at most S.
The history has at most W errors. Therefore its sum has magnitude at most
W*S=65,536,000,000, below i64::MAX. The derivative magnitude is at most 2*S.

Each gain is at most 2^64−1. Damping multiplies it by S before division, so the
unsigned intermediate is below 2^84. Its denominator is at least S and below
2^65. The result cannot exceed the original gain and fits u64.

Before division, the largest signed product is less than 2^64*2^36=2^100.
The sum of the base and three such bounds is below 2^102, below i128::MAX.
Thus the signed calculations cannot overflow under the admitted configuration.
The reference script also checks a conservative integer bound directly.

Clamping the command proves E_min <= E_command <= E_max at every accepted
transition. This is a per-epoch bound. It does not bound cumulative supply.
Removing one error before inserting into a full window preserves its length.
The next epoch must be representable. All fallible checks precede state mutation.
Induction proves these properties for every finite accepted input sequence.
Integer operations and explicit update order give identical outputs for identical
validated state and input. Allocation failure or process termination is outside
this functional proof; durable transaction recovery remains an integration task.

## 2. Conditional stability theorem for the full controller

Normalize emission as x=E/E_ref, for a fixed positive E_ref measured in uDRT.
Let x_base=E_base/E_ref and y_t=u_t−u_target. Let the plant update be
u_(t+1)=F(u_t,x_t)+w_t. F includes projection onto [0,1].
The term w_t includes bounded modeling, input-quantization, and observation errors.

Assume the following conditions over the entire admitted operating domain:

- F(u_target,x_base)=u_target. The base emission supports the target equilibrium.
- |F(u,x)−F(u_target,x_base)| <= a*|u−u_target| + b*|x−x_base|,
  with known finite a,b >= 0.
- |w_t| <= d. The plant domain and its bounds apply after disturbances.
- The target, gain tables, limits, window, and E_ref stay fixed.
- Each epoch supplies one valid observation. Issuance x_t is the prior epoch's
  command. After startup there is no additional unknown actuation delay.
- Utilization in the controller and plant is the same normalized observation.
  Any discrepancy must be included in the disturbance model.

For j in {p,i,d}, let k_j be the maximum of the soft and hard coefficients,
divided by E_ref. Define C=k_p+W*k_i+2*k_d and rho=a+b*C.
If rho<1, the undisturbed loop converges to the target equilibrium.
With bounded disturbances, limsup |y_t| <= d/(1−rho).

### Proof

The integral projection contains zero, so its magnitude does not exceed the
sum of the absolute errors. Damping never increases a gain. Truncation toward
zero never increases the magnitude of a term. These facts hold for either
regime and for any nonnegative volatility sequence.

Because E_base lies inside the emission interval, projecting the raw command
onto that interval cannot increase its distance from E_base. The triangle
inequality therefore gives

|x_(t+1)−x_base| <= C * max(|y_t|,...,|y_(t−max(W−1,1))|).

This inequality holds across regime changes. It does not require the switched
controller to be continuous. It compares each command to the common zero-error
equilibrium, rather than comparing arbitrary pairs of trajectories.

Let L=max(W,2) and M_t=max(|y_t|,...,|y_(t−L)|). The one-epoch actuation delay
and plant bound imply |y_(t+1)| <= rho*M_t+d.
After finite startup, the complete error history required by this inequality
exists. Any finite initial command contributes only a finite initial bound.

For d=0, M cannot increase. After L+1 steps, all old samples leave this maximum,
so M_(t+L+1) <= rho*M_t. Repeating this inequality proves convergence.

For d>=0, set B=d/(1−rho) and N_t=max(0,M_t−B). New samples have excess above
B at most rho*N_t. Thus N_(t+L+1) <= rho*N_t. This proves the stated limit bound.
The emission bound above also gives limsup |x_t−x_base| <= C*B.
This completes the conditional proof, including switching, both clamps,
finite-window memory, arbitrary gain damping, and the specified integer rounding.

### A sufficient bound for the paper plant

Consider F(u,x)=clip_[0,1](beta*u+(1−beta)*D'(x,P(u))), with
D'=clip_[0,1](A*x^eta*(P/P_ref)^(-mu)). Require 0<=beta<1,
A>0, eta,mu>=0, x>=x_min>0, P>0, and a continuous price function satisfying
|log P(u)−log P(v)| <= L_P*|u−v|.

Within unclipped demand, D'<=1. Its derivative magnitudes are at most
mu*L_P in u and eta/x_min in x. The demand clamp preserves these Lipschitz
bounds across its boundaries. The outer projection is also nonexpansive.
Therefore one sufficient choice is
a=beta+(1−beta)*mu*L_P and b=(1−beta)*eta/x_min.
An exponential price with fixed nonnegative alpha and fixed positive floor/cap
has L_P<=alpha. Time-varying prices require uniform bounds and an equilibrium
or disturbance analysis that includes their variation.

For illustration only, beta=1/2, eta=mu=1, L_P=1/10, x_min=1/2 yield
a=11/20 and b=1. Maximum normalized gains 1/5,1/100,1/50 with W=3 yield
C=27/100 and rho=41/50<1. This shows that the sufficient region is nonempty.
It does not estimate the Dytallix plant. A must also satisfy the equilibrium
condition. The test-vector gain table is separate and fails this example bound.

## 3. Claims that remain unproved

The certificate is sufficient, not necessary. Failure to satisfy it does not
prove instability. A less conservative proof may admit other calibrated gains.
The exact rational checker verifies the supplied inequality. It cannot establish
that a,b or the equilibrium assumption describe an actual network or market.

The paper's Gaussian disturbance has unbounded support. A finite d theorem does
not establish its proposed stochastic claim. Use a justified bounded model or
complete a separate probabilistic analysis. Physical utilization clipping alone
does not imply a small disturbance bound or useful tracking accuracy.

The finite window does not guarantee zero offset if E_base fails the equilibrium
condition. No theorem here proves gain optimality, a token price floor, validator
solvency, manipulation resistance, or stable governance parameter changes.
No theorem here approves D02–D07, supplies oracle trust assumptions, or proves
atomic issuance accounting. Independent mathematical review remains required.
