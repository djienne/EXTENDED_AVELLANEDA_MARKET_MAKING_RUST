//! GARCH(1,1) volatility estimation module
//!
//! Implements a GARCH(1,1) model with constant mean for volatility forecasting.
//! The model is:
//!   r_t = μ + ε_t
//!   ε_t = σ_t * z_t,  z_t ~ N(0,1)
//!   σ²_t = ω + α*ε²_{t-1} + β*σ²_{t-1}
//!
//! Parameters:
//!   μ (mu)    : mean return
//!   ω (omega) : baseline variance (must be > 0)
//!   α (alpha) : ARCH effect coefficient (reaction to shocks, ≥ 0)
//!   β (beta)  : GARCH effect coefficient (persistence, ≥ 0)
//!   Constraint: α + β < 1 for stationarity

use anyhow::{anyhow, Result};
use argmin::core::{CostFunction, Error, Executor, State};
use argmin::solver::neldermead::NelderMead;

/// Constants for numerical stability and optimization
const SMALL_POS: f64 = 1e-12;
const LARGE_NUMBER: f64 = 1e12;
const PI: f64 = std::f64::consts::PI;

/// GARCH(1,1) fitted parameters (Gaussian distribution)
#[derive(Debug, Clone)]
pub struct GarchParams {
    pub mu: f64,    // Mean return
    pub omega: f64, // Baseline variance
    pub alpha: f64, // ARCH coefficient
    pub beta: f64,  // GARCH coefficient
}

impl GarchParams {
    /// Check if parameters satisfy GARCH constraints
    pub fn is_valid(&self) -> bool {
        self.omega > 0.0
            && self.alpha >= 0.0
            && self.beta >= 0.0
            && (self.alpha + self.beta) < 1.0
    }

    /// Get persistence (α + β)
    pub fn persistence(&self) -> f64 {
        self.alpha + self.beta
    }
}

/// GARCH(1,1) fitted parameters with Student's t distribution
#[derive(Debug, Clone)]
pub struct GarchParamsStudentT {
    pub mu: f64,    // Mean return
    pub omega: f64, // Baseline variance
    pub alpha: f64, // ARCH coefficient
    pub beta: f64,  // GARCH coefficient
    pub nu: f64,    // Degrees of freedom (must be > 2)
}

impl GarchParamsStudentT {
    /// Check if parameters satisfy GARCH constraints
    pub fn is_valid(&self) -> bool {
        self.omega > 0.0
            && self.alpha >= 0.0
            && self.beta >= 0.0
            && (self.alpha + self.beta) < 1.0
            && self.nu > 2.0  // Need nu > 2 for finite variance
    }

    /// Get persistence (α + β)
    pub fn persistence(&self) -> f64 {
        self.alpha + self.beta
    }
}

/// GARCH(1,1) one-step-ahead forecast (Gaussian)
#[derive(Debug, Clone)]
pub struct GarchForecast {
    pub params: GarchParams,   // Fitted parameters
    pub mean_next: f64,        // Predicted mean return for next period
    pub sigma_next: f64,       // Predicted volatility (std dev) for next period
    pub var_next: f64,         // Predicted variance for next period
}

/// GARCH(1,1) one-step-ahead forecast (Student's t)
#[derive(Debug, Clone)]
pub struct GarchForecastStudentT {
    pub params: GarchParamsStudentT,  // Fitted parameters
    pub mean_next: f64,               // Predicted mean return for next period
    pub sigma_next: f64,              // Predicted volatility (std dev) for next period
    pub var_next: f64,                // Predicted variance for next period
}

/// Cost function for GARCH parameter estimation (Gaussian, negative log-likelihood)
struct GarchCostFunction<'a> {
    returns: &'a [f64],
}

impl<'a> CostFunction for GarchCostFunction<'a> {
    type Param = Vec<f64>;
    type Output = f64;

    fn cost(&self, theta: &Self::Param) -> Result<Self::Output, Error> {
        Ok(negative_log_likelihood(theta, self.returns))
    }
}

/// Cost function for GARCH parameter estimation (Student's t, negative log-likelihood)
struct GarchCostFunctionStudentT<'a> {
    returns: &'a [f64],
}

impl<'a> CostFunction for GarchCostFunctionStudentT<'a> {
    type Param = Vec<f64>;
    type Output = f64;

    fn cost(&self, theta: &Self::Param) -> Result<Self::Output, Error> {
        Ok(negative_log_likelihood_studentt(theta, self.returns))
    }
}

/// Calculate mean of an array
fn array_mean(x: &[f64]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    x.iter().sum::<f64>() / x.len() as f64
}

/// Calculate sample variance of an array (divides by N-1)
fn array_variance(x: &[f64]) -> f64 {
    if x.len() <= 1 {
        return 0.0;
    }
    let mean = array_mean(x);
    let sum_sq: f64 = x.iter().map(|v| (v - mean).powi(2)).sum();
    sum_sq / (x.len() - 1) as f64
}

/// Compute log-gamma function using Lanczos approximation
/// Accurate for x > 0.5
fn log_gamma(x: f64) -> f64 {
    // Lanczos approximation coefficients (g=7, n=9)
    const G: f64 = 7.0;
    const COEF: [f64; 9] = [
        0.99999999999980993,
        676.5203681218851,
        -1259.1392167224028,
        771.32342877765313,
        -176.61502916214059,
        12.507343278686905,
        -0.13857109526572012,
        9.9843695780195716e-6,
        1.5056327351493116e-7,
    ];

    if x < 0.5 {
        // Use reflection formula for small x
        PI.ln() - (PI * x).sin().abs().ln() - log_gamma(1.0 - x)
    } else {
        let z = x - 1.0;
        let mut sum = COEF[0];
        for i in 1..9 {
            sum += COEF[i] / (z + i as f64);
        }
        let temp = z + G + 0.5;
        (2.0 * PI).sqrt().ln() + (z + 0.5) * temp.ln() - temp + sum.ln()
    }
}

/// Compute negative log-likelihood for GARCH(1,1) model
///
/// # Arguments
/// * `theta` - Parameter vector [mu, omega, alpha, beta]
/// * `returns` - Training returns data
///
/// # Returns
/// Negative log-likelihood value (to be minimized)
fn negative_log_likelihood(theta: &[f64], returns: &[f64]) -> f64 {
    // Extract parameters
    let mu = theta[0];
    let omega = theta[1];
    let alpha = theta[2];
    let beta = theta[3];

    // Enforce constraints
    if omega <= 0.0 || alpha < 0.0 || beta < 0.0 || (alpha + beta) >= 1.0 {
        return LARGE_NUMBER;
    }

    let n = returns.len();
    if n < 2 {
        return LARGE_NUMBER;
    }

    // Compute residuals: ε_t = r_t - μ
    let residuals: Vec<f64> = returns.iter().map(|r| r - mu).collect();

    // Initialize conditional variance with sample variance
    let mut sigma2 = vec![0.0; n];
    let sample_var = array_variance(returns);
    sigma2[0] = if sample_var > 0.0 { sample_var } else { SMALL_POS };

    // GARCH recursion: σ²_t = ω + α*ε²_{t-1} + β*σ²_{t-1}
    for t in 1..n {
        sigma2[t] = omega + alpha * residuals[t - 1].powi(2) + beta * sigma2[t - 1];

        if sigma2[t] <= 0.0 {
            return LARGE_NUMBER;
        }
    }

    // Gaussian negative log-likelihood:
    // NLL = Σ [0.5*log(2π) + 0.5*log(σ²_t) + 0.5*ε²_t/σ²_t]
    let mut nll = 0.0;
    let c = 0.5 * (2.0 * PI).ln();

    for t in 0..n {
        nll += c + 0.5 * sigma2[t].ln() + 0.5 * residuals[t].powi(2) / sigma2[t];
    }

    nll
}

/// Compute negative log-likelihood for GARCH(1,1) model with Student's t distribution
///
/// # Arguments
/// * `theta` - Parameter vector [mu, omega, alpha, beta, nu]
/// * `returns` - Training returns data
///
/// # Returns
/// Negative log-likelihood value (to be minimized)
fn negative_log_likelihood_studentt(theta: &[f64], returns: &[f64]) -> f64 {
    // Extract parameters
    let mu = theta[0];
    let omega = theta[1];
    let alpha = theta[2];
    let beta = theta[3];
    let nu = theta[4];

    // Enforce constraints
    if omega <= 0.0 || alpha < 0.0 || beta < 0.0 || (alpha + beta) >= 1.0 || nu <= 2.0 {
        return LARGE_NUMBER;
    }

    let n = returns.len();
    if n < 2 {
        return LARGE_NUMBER;
    }

    // Compute residuals: ε_t = r_t - μ
    let residuals: Vec<f64> = returns.iter().map(|r| r - mu).collect();

    // Initialize conditional variance with sample variance
    let mut sigma2 = vec![0.0; n];
    let sample_var = array_variance(returns);
    sigma2[0] = if sample_var > 0.0 { sample_var } else { SMALL_POS };

    // GARCH recursion: σ²_t = ω + α*ε²_{t-1} + β*σ²_{t-1}
    for t in 1..n {
        sigma2[t] = omega + alpha * residuals[t - 1].powi(2) + beta * sigma2[t - 1];

        if sigma2[t] <= 0.0 {
            return LARGE_NUMBER;
        }
    }

    // Student's t negative log-likelihood (standardized with Var=1):
    // NLL = -Σ [log(Γ((ν+1)/2)) - log(Γ(ν/2)) - 0.5*log((ν-2)π) - 0.5*log(σ²_t)
    //           - ((ν+1)/2)*log(1 + ε²_t/((ν-2)*σ²_t))]
    let mut nll = 0.0;
    let log_gamma_nu_plus_1_over_2 = log_gamma((nu + 1.0) / 2.0);
    let log_gamma_nu_over_2 = log_gamma(nu / 2.0);
    let c = log_gamma_nu_plus_1_over_2 - log_gamma_nu_over_2 - 0.5 * ((nu - 2.0) * PI).ln();

    for t in 0..n {
        let z_squared = residuals[t].powi(2) / sigma2[t];
        let term = ((nu + 1.0) / 2.0) * (1.0 + z_squared / (nu - 2.0)).ln();
        nll -= c - 0.5 * sigma2[t].ln() - term;
    }

    nll
}

/// Fit GARCH(1,1) model to returns data using maximum likelihood estimation
///
/// # Arguments
/// * `returns` - Log returns data (should be last N observations for training)
///
/// # Returns
/// Fitted GARCH parameters
///
/// # Errors
/// Returns error if optimization fails or data is insufficient
pub fn fit_garch_11(returns: &[f64]) -> Result<GarchParams> {
    // Input validation
    if returns.len() < 3 {
        return Err(anyhow!(
            "Need at least 3 returns for GARCH(1,1) estimation, got {}",
            returns.len()
        ));
    }

    // Check for non-finite values
    if returns.iter().any(|r| !r.is_finite()) {
        return Err(anyhow!("Returns contain non-finite values (NaN or Inf)"));
    }

    // Initial parameter guesses
    let mu0 = array_mean(returns);
    let v0 = array_variance(returns);
    let v0 = if v0 > 0.0 { v0 } else { SMALL_POS };

    let omega0 = 0.1 * v0;
    let alpha0 = 0.05;
    let beta0 = 0.90;

    // Create initial simplex for Nelder-Mead (n+1 = 5 vertices for 4 parameters)
    let theta0 = vec![mu0, omega0, alpha0, beta0];
    let mut initial_params = vec![theta0.clone()];

    // Create simplex vertices by perturbing each parameter
    for i in 0..4 {
        let mut perturbed = theta0.clone();
        match i {
            0 => perturbed[i] *= 1.1,              // mu
            1 => perturbed[i] *= 1.2,              // omega
            2 => perturbed[i] = 0.08,              // alpha
            3 => perturbed[i] = 0.85,              // beta
            _ => {}
        }
        initial_params.push(perturbed);
    }

    // Set up cost function
    let cost = GarchCostFunction { returns };

    // Set up Nelder-Mead solver (derivative-free)
    let solver = NelderMead::new(initial_params)
        .with_sd_tolerance(1e-6)?;

    // Run optimization
    let result = Executor::new(cost, solver)
        .configure(|state| state.max_iters(5000))
        .run()
        .map_err(|e| anyhow!("GARCH optimization failed: {}", e))?;

    // Extract optimized parameters
    let theta_hat = result.state().get_best_param().ok_or_else(|| {
        anyhow!("GARCH optimization did not produce parameters")
    })?;

    let params = GarchParams {
        mu: theta_hat[0],
        omega: theta_hat[1],
        alpha: theta_hat[2],
        beta: theta_hat[3],
    };

    // Validate final parameters
    if !params.is_valid() {
        return Err(anyhow!(
            "GARCH optimization produced invalid parameters: μ={:.6}, ω={:.6}, α={:.6}, β={:.6}, α+β={:.6}",
            params.mu, params.omega, params.alpha, params.beta, params.persistence()
        ));
    }

    Ok(params)
}

/// Predict one-step-ahead volatility using fitted GARCH(1,1) parameters
///
/// # Arguments
/// * `params` - Fitted GARCH parameters
/// * `returns` - Historical returns data (same as used for fitting)
///
/// # Returns
/// One-step-ahead forecast (mean, volatility, variance)
///
/// # Errors
/// Returns error if data is insufficient or parameters are invalid
pub fn predict_one_step(params: &GarchParams, returns: &[f64]) -> Result<GarchForecast> {
    // Validate parameters
    if !params.is_valid() {
        return Err(anyhow!("Invalid GARCH parameters"));
    }

    // Validate data
    if returns.len() < 2 {
        return Err(anyhow!(
            "Need at least 2 returns for prediction, got {}",
            returns.len()
        ));
    }

    let n = returns.len();

    // Recompute residuals with fitted mean
    let residuals: Vec<f64> = returns.iter().map(|r| r - params.mu).collect();

    // Recompute conditional variances with fitted parameters
    let mut sigma2 = vec![0.0; n];
    let sample_var = array_variance(returns);
    sigma2[0] = if sample_var > 0.0 { sample_var } else { SMALL_POS };

    for t in 1..n {
        sigma2[t] = params.omega
                    + params.alpha * residuals[t - 1].powi(2)
                    + params.beta * sigma2[t - 1];

        if sigma2[t] <= 0.0 {
            sigma2[t] = SMALL_POS;
        }
    }

    // One-step-ahead forecast:
    // σ²_{n+1|n} = ω + α*ε²_n + β*σ²_n
    let eps_last = residuals[n - 1];
    let sigma2_last = sigma2[n - 1];

    let var_next = params.omega
                   + params.alpha * eps_last.powi(2)
                   + params.beta * sigma2_last;

    let var_next = if var_next > 0.0 { var_next } else { SMALL_POS };
    let sigma_next = var_next.sqrt();
    let mean_next = params.mu; // Constant mean assumption

    Ok(GarchForecast {
        params: params.clone(),
        mean_next,
        sigma_next,
        var_next,
    })
}

/// Fit GARCH(1,1) model with Student's t distribution
///
/// # Arguments
/// * `returns` - Log returns data (should be last N observations for training)
///
/// # Returns
/// Fitted GARCH parameters with degrees of freedom
///
/// # Errors
/// Returns error if optimization fails or data is insufficient
pub fn fit_garch_11_studentt(returns: &[f64]) -> Result<GarchParamsStudentT> {
    // Input validation
    if returns.len() < 3 {
        return Err(anyhow!(
            "Need at least 3 returns for GARCH(1,1) estimation, got {}",
            returns.len()
        ));
    }

    // Check for non-finite values
    if returns.iter().any(|r| !r.is_finite()) {
        return Err(anyhow!("Returns contain non-finite values (NaN or Inf)"));
    }

    // Initial parameter guesses
    let mu0 = array_mean(returns);
    let v0 = array_variance(returns);
    let v0 = if v0 > 0.0 { v0 } else { SMALL_POS };

    let omega0 = 0.1 * v0;
    let alpha0 = 0.05;
    let beta0 = 0.90;
    let nu0 = 6.0;  // Initial degrees of freedom

    // Create initial simplex for Nelder-Mead (n+1 = 6 vertices for 5 parameters)
    let theta0 = vec![mu0, omega0, alpha0, beta0, nu0];
    let mut initial_params = vec![theta0.clone()];

    // Create simplex vertices by perturbing each parameter
    for i in 0..5 {
        let mut perturbed = theta0.clone();
        match i {
            0 => perturbed[i] *= 1.1,              // mu
            1 => perturbed[i] *= 1.2,              // omega
            2 => perturbed[i] = 0.08,              // alpha
            3 => perturbed[i] = 0.85,              // beta
            4 => perturbed[i] = 8.0,               // nu
            _ => {}
        }
        initial_params.push(perturbed);
    }

    // Set up cost function
    let cost = GarchCostFunctionStudentT { returns };

    // Set up Nelder-Mead solver (derivative-free)
    let solver = NelderMead::new(initial_params)
        .with_sd_tolerance(1e-6)?;

    // Run optimization
    let result = Executor::new(cost, solver)
        .configure(|state| state.max_iters(5000))
        .run()
        .map_err(|e| anyhow!("GARCH Student's t optimization failed: {}", e))?;

    // Extract optimized parameters
    let theta_hat = result.state().get_best_param().ok_or_else(|| {
        anyhow!("GARCH Student's t optimization did not produce parameters")
    })?;

    let params = GarchParamsStudentT {
        mu: theta_hat[0],
        omega: theta_hat[1],
        alpha: theta_hat[2],
        beta: theta_hat[3],
        nu: theta_hat[4],
    };

    // Validate final parameters
    if !params.is_valid() {
        return Err(anyhow!(
            "GARCH Student's t optimization produced invalid parameters: μ={:.6}, ω={:.6}, α={:.6}, β={:.6}, ν={:.2}, α+β={:.6}",
            params.mu, params.omega, params.alpha, params.beta, params.nu, params.persistence()
        ));
    }

    Ok(params)
}

/// Predict one-step-ahead volatility using fitted GARCH(1,1) Student's t parameters
///
/// # Arguments
/// * `params` - Fitted GARCH parameters with Student's t distribution
/// * `returns` - Historical returns data (same as used for fitting)
///
/// # Returns
/// One-step-ahead forecast (mean, volatility, variance)
///
/// # Errors
/// Returns error if data is insufficient or parameters are invalid
pub fn predict_one_step_studentt(params: &GarchParamsStudentT, returns: &[f64]) -> Result<GarchForecastStudentT> {
    // Validate parameters
    if !params.is_valid() {
        return Err(anyhow!("Invalid GARCH Student's t parameters"));
    }

    // Validate data
    if returns.len() < 2 {
        return Err(anyhow!(
            "Need at least 2 returns for prediction, got {}",
            returns.len()
        ));
    }

    let n = returns.len();

    // Recompute residuals with fitted mean
    let residuals: Vec<f64> = returns.iter().map(|r| r - params.mu).collect();

    // Recompute conditional variances with fitted parameters
    let mut sigma2 = vec![0.0; n];
    let sample_var = array_variance(returns);
    sigma2[0] = if sample_var > 0.0 { sample_var } else { SMALL_POS };

    for t in 1..n {
        sigma2[t] = params.omega
                    + params.alpha * residuals[t - 1].powi(2)
                    + params.beta * sigma2[t - 1];

        if sigma2[t] <= 0.0 {
            sigma2[t] = SMALL_POS;
        }
    }

    // One-step-ahead forecast:
    // σ²_{n+1|n} = ω + α*ε²_n + β*σ²_n
    let eps_last = residuals[n - 1];
    let sigma2_last = sigma2[n - 1];

    let var_next = params.omega
                   + params.alpha * eps_last.powi(2)
                   + params.beta * sigma2_last;

    let var_next = if var_next > 0.0 { var_next } else { SMALL_POS };
    let sigma_next = var_next.sqrt();
    let mean_next = params.mu; // Constant mean assumption

    Ok(GarchForecastStudentT {
        params: params.clone(),
        mean_next,
        sigma_next,
        var_next,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_array_mean() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((array_mean(&data) - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_array_variance() {
        let data = vec![2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let var = array_variance(&data);
        // Sample variance = 4.571428...
        assert!((var - 4.571428).abs() < 0.001);
    }

    #[test]
    fn test_garch_params_validation() {
        let valid = GarchParams {
            mu: 0.0001,
            omega: 0.00001,
            alpha: 0.05,
            beta: 0.90,
        };
        assert!(valid.is_valid());
        assert!((valid.persistence() - 0.95).abs() < 1e-10);

        let invalid_persistence = GarchParams {
            mu: 0.0001,
            omega: 0.00001,
            alpha: 0.7,
            beta: 0.5,
        };
        assert!(!invalid_persistence.is_valid());

        let invalid_omega = GarchParams {
            mu: 0.0001,
            omega: -0.00001,
            alpha: 0.05,
            beta: 0.90,
        };
        assert!(!invalid_omega.is_valid());
    }

    #[test]
    fn test_garch_fit_simple() {
        // Generate simple synthetic data
        let mut returns = Vec::new();
        let mu = 0.0001;
        let sigma = 0.02;

        for i in 0..100 {
            let t = i as f64 * 0.1;
            let ret = mu + sigma * (t * 0.1).sin();
            returns.push(ret);
        }

        let result = fit_garch_11(&returns);
        assert!(result.is_ok());

        let params = result.unwrap();
        assert!(params.is_valid());
        assert!(params.persistence() < 1.0);
        assert!(params.omega > 0.0);
    }

    #[test]
    fn test_predict_one_step() {
        // Simple test data
        let returns = vec![0.001, -0.002, 0.003, -0.001, 0.002];

        let params = GarchParams {
            mu: 0.0005,
            omega: 0.00001,
            alpha: 0.05,
            beta: 0.90,
        };

        let forecast = predict_one_step(&params, &returns);
        assert!(forecast.is_ok());

        let forecast = forecast.unwrap();
        assert!(forecast.sigma_next > 0.0);
        assert!(forecast.var_next > 0.0);
        assert!((forecast.sigma_next.powi(2) - forecast.var_next).abs() < 1e-10);
    }
}
