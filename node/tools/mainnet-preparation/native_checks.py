"""Scalar and native timing checks reused from sealed development preparation.
The selected input checks are stricter than permissive native JSON decoding.
This module contains no genesis generator or activation function.
"""
import json
import re
U128_MAX = 2**128 - 1
U64_MAX = 2**64 - 1

def require(ok, message):
    if not ok:
        raise ValueError(message)

def exact(obj, fields):
    require(type(obj) is dict and set(obj) == set(fields.split()), 'Missing or unknown contract fields')

def unique(pairs):
    result = {}
    for k, v in pairs:
        require(k not in result, 'Duplicate JSON key')
        result[k] = v
    return result

def canonical(data):
    return (json.dumps(data, sort_keys=True, ensure_ascii=True, separators=(',', ':'), allow_nan=False) + '\n').encode()

def amount(value):
    require(type(value) is str and re.fullmatch(r'0|[1-9][0-9]{0,38}', value) is not None,
            'Amount must be a canonical decimal string')
    n = int(value)
    require(n <= U128_MAX, 'Amount exceeds u128')
    return n

def uint(value, maximum=U64_MAX):
    require(type(value) is int and 0 <= value <= maximum, 'Invalid unsigned integer')
    return value

def identity(value):
    require(type(value) is str and 0 < len(value) <= 256 and
            all(32 < ord(c) < 127 for c in value), 'Invalid fixture identity')
    return value

def vesting(value, allocation):
    require(type(value) is dict, 'Explicit vesting object required')
    if value.get('kind') == 'unlocked':
        exact(value, 'kind')
    elif value.get('kind') == 'linear_after_cliff':
        exact(value, 'kind total_amount start_time cliff_duration vesting_duration allow_staking')
        require(amount(value['total_amount']) == allocation and allocation > 0, 'Vesting allocation differs')
        start, cliff, duration = [uint(value[k]) for k in ('start_time', 'cliff_duration', 'vesting_duration')]
        require(cliff < duration and start + duration <= U64_MAX, 'Invalid vesting time range')
        require(type(value['allow_staking']) is bool, 'Explicit staking permission required')
    else:
        raise ValueError('Unsupported vesting kind')

def validate_timing(timing):
    exact(timing, 'version profile decimals epoch_blocks initial_epoch_budget_udrt controller max_recorded_epochs')
    require(type(timing['version']) is int and timing['version'] == 1 and
            type(timing['decimals']) is int and timing['decimals'] == 6 and
            timing['profile'] == 'development', 'Explicit development issuance input required')
    require(uint(timing['epoch_blocks']) > 0 and 1 <= uint(timing['max_recorded_epochs']) <= 1000000,
            'Invalid epoch or journal bound')
    initial = amount(timing['initial_epoch_budget_udrt'])
    require(initial <= U64_MAX, 'Initial issuance budget exceeds u64')
    c = timing['controller']
    exact(c, 'target_ppm shock_threshold_ppm volatility_threshold_ppm window_samples integral_min integral_max soft hard base_udrt min_udrt max_udrt')
    for key in 'target_ppm shock_threshold_ppm volatility_threshold_ppm window_samples base_udrt min_udrt max_udrt'.split():
        uint(c[key])
    require(c['target_ppm'] <= 1000000 and 1 <= c['shock_threshold_ppm'] <= 1000000 and
            1 <= c['window_samples'] <= 65536 and c['min_udrt'] <= c['base_udrt'] <= c['max_udrt'] and
            c['min_udrt'] <= initial <= c['max_udrt'], 'Invalid controller bounds')
    bound = c['window_samples'] * 1000000
    require(type(c['integral_min']) is int and type(c['integral_max']) is int and
            -bound <= c['integral_min'] <= 0 <= c['integral_max'] <= bound, 'Invalid controller integral bounds')
    for name in ('soft', 'hard'):
        exact(c[name], 'proportional integral derivative')
        for value in c[name].values():
            uint(value)
