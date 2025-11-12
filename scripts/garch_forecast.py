#!/usr/bin/env python3
"""
GARCH(1,1) volatility forecasting using the arch library.

This script fits a GARCH(1,1) model to returns data and provides
a one-step-ahead volatility forecast.

Usage:
    python scripts/garch_forecast.py <returns_file> [distribution]

Where:
    returns_file is a CSV with one return per line.
    distribution is optional: 'studentst' (default) or 'normal'
"""

import sys
import numpy as np
import json
import warnings

# Suppress convergence warnings to avoid breaking JSON output
warnings.filterwarnings('ignore')


def fit_garch_and_forecast(returns, distribution='studentst', starting_values=None):
    """
    Fit GARCH(1,1) model and return one-step-ahead forecast.

    Parameters
    ----------
    returns : array-like
        Log returns data
    distribution : str
        Distribution to use: 'studentst' or 'normal'

    Returns
    -------
    dict with keys:
        - mu: mean return
        - omega: baseline variance
        - alpha: ARCH coefficient (alpha[1])
        - beta: GARCH coefficient (beta[1])
        - sigma_next: one-step-ahead volatility forecast (per-step)
        - var_next: one-step-ahead variance forecast
        - success: True if fitting succeeded
        - message: error message if failed
    """
    try:
        # Import arch here to catch import errors
        from arch import arch_model
        from arch.univariate.base import ConvergenceWarning

        # Suppress convergence warnings specifically
        warnings.filterwarnings('ignore', category=ConvergenceWarning)

        returns = np.asarray(returns, dtype=np.float64)

        if len(returns) < 3:
            return {
                'success': False,
                'message': f'Need at least 3 returns, got {len(returns)}'
            }

        # Check for non-finite values
        if not np.all(np.isfinite(returns)):
            return {
                'success': False,
                'message': 'Returns contain non-finite values (NaN or Inf)'
            }

        # Fit GARCH(1,1) model with constant mean
        model = arch_model(
            returns,
            mean='Constant',
            vol='GARCH',
            p=1,
            q=1,
            dist=distribution,
            rescale=False
        )

        # Fit model with starting values if provided
        try:
            # Build fit parameters
            fit_kwargs = {
                'disp': 'off',
                'show_warning': False,
                'update_freq': 0
            }

            if starting_values is not None:
                # Try multiple random perturbations and pick the best likelihood
                num_trials = 100
                best_result = None
                best_loglik = -np.inf

                for trial in range(num_trials):
                    # starting_values = [mu, omega, alpha, beta]
                    # Add random perturbation to explore parameter space
                    sv = np.array(starting_values, dtype=float)

                    # Multiply each parameter by random factor between 0.125x and 8.0x
                    np.random.seed(None)  # Use different seed each time
                    multiplier = np.random.uniform(0.125, 8.0, size=len(sv))
                    sv = sv * multiplier

                    # Ensure parameters stay within valid bounds
                    sv[1] = max(sv[1], 1e-8)  # omega must be positive
                    sv[2] = np.clip(sv[2], 0.01, 0.99)  # alpha between 0.01 and 0.99
                    sv[3] = np.clip(sv[3], 0.01, 0.99)  # beta between 0.01 and 0.99

                    if distribution == 'studentst':
                        # Add degrees of freedom with randomization (between 3 and 20)
                        nu_random = np.random.uniform(3.0, 20.0)
                        sv = np.append(sv, nu_random)

                    trial_fit_kwargs = fit_kwargs.copy()
                    trial_fit_kwargs['starting_values'] = sv
                    trial_fit_kwargs['options'] = {'ftol': 1e-6, 'maxiter': 20000}

                    try:
                        trial_result = model.fit(**trial_fit_kwargs)
                        loglik = trial_result.loglikelihood

                        # Keep the best result
                        if loglik > best_loglik:
                            best_loglik = loglik
                            best_result = trial_result
                    except:
                        # If this trial fails, skip it
                        continue

                if best_result is None:
                    raise Exception("All trials failed to converge")

                result = best_result
            else:
                # No starting values provided, use defaults
                result = model.fit(**fit_kwargs)
        except Exception as e:
            return {
                'success': False,
                'message': f'Model fit failed: {str(e)}'
            }

        # Extract parameters
        # result.params gives us: ['mu', 'omega', 'alpha[1]', 'beta[1]', 'nu']
        # where 'nu' is the degrees of freedom for Student's t distribution
        # rescale=True handles scaling automatically, so parameters are in original scale
        params = result.params
        mu = params['mu']
        omega = params['omega']
        alpha = params['alpha[1]']
        beta = params['beta[1]']
        nu = params['nu'] if 'nu' in params else None  # Degrees of freedom

        # Get one-step-ahead forecast
        # forecast() returns ForecastResult with:
        #   - mean: forecasted mean
        #   - variance: forecasted variance
        #   - residual_variance: forecasted conditional variance
        forecast = result.forecast(horizon=1, start=None, reindex=False)

        # Get the forecasted variance (this is σ²_{t+1|t})
        # The variance attribute contains conditional variance forecasts
        # rescale=True handles scaling automatically, so forecast is in original scale
        var_next = float(forecast.variance.iloc[-1, 0])
        sigma_next = np.sqrt(var_next)

        result_dict = {
            'success': True,
            'mu': float(mu),
            'omega': float(omega),
            'alpha': float(alpha),
            'beta': float(beta),
            'sigma_next': float(sigma_next),
            'var_next': float(var_next),
            'log_likelihood': float(result.loglikelihood),
            'aic': float(result.aic),
            'bic': float(result.bic),
            'convergence_flag': int(result.convergence_flag) if hasattr(result, 'convergence_flag') else -1,
            'num_iterations': int(result.iterations) if hasattr(result, 'iterations') else -1,
        }

        # Add degrees of freedom if using Student's t
        if nu is not None:
            result_dict['nu'] = float(nu)

        return result_dict

    except ImportError:
        return {
            'success': False,
            'message': 'arch library not installed. Install with: pip install arch'
        }
    except Exception as e:
        return {
            'success': False,
            'message': f'GARCH fitting failed: {str(e)}'
        }


def main():
    """Command-line interface."""
    if len(sys.argv) < 2:
        print("Usage: python scripts/garch_forecast.py <returns_file> [distribution] [mu omega alpha beta]")
        print("\nWhere:")
        print("  returns_file is a CSV/text file with one return per line")
        print("  distribution is optional: 'studentst' (default) or 'normal'")
        print("  mu omega alpha beta are optional starting values from Rust GARCH fit")
        sys.exit(1)

    returns_file = sys.argv[1]
    distribution = 'studentst'
    starting_values = None

    # Parse distribution (if provided and not a number)
    if len(sys.argv) >= 3:
        try:
            float(sys.argv[2])  # Check if it's a number (starting value)
            # It's a number, so no distribution was specified
            distribution = 'studentst'
            starting_values = [float(x) for x in sys.argv[2:6]] if len(sys.argv) >= 6 else None
        except ValueError:
            # It's a string (distribution)
            distribution = sys.argv[2]
            if len(sys.argv) >= 7:
                starting_values = [float(x) for x in sys.argv[3:7]]

    # Validate distribution
    if distribution not in ['studentst', 'normal']:
        print(f"Error: Invalid distribution '{distribution}'", file=sys.stderr)
        print("Valid options: 'studentst', 'normal'", file=sys.stderr)
        sys.exit(1)

    # Load returns from file
    try:
        returns = np.loadtxt(returns_file)
    except Exception as e:
        print(f"Error loading returns file: {e}", file=sys.stderr)
        sys.exit(1)

    # Fit GARCH and forecast
    result = fit_garch_and_forecast(returns, distribution, starting_values)

    # Output results as JSON
    print(json.dumps(result, indent=2))

    if not result['success']:
        sys.exit(1)


if __name__ == '__main__':
    main()
