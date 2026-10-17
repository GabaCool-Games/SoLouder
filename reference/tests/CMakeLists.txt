cmake_minimum_required(VERSION 3.16)
project(solouder_reference_tests CXX)

# Regression tests for the patched C++ SoLoud reference. They run against the
# checkout produced by scripts/fetch_reference.py (reference/soloud), or any
# other SoLoud tree passed with -DSOLOUDER_REFERENCE_DIR=<path>.
set(SOLOUDER_REFERENCE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../soloud"
    CACHE PATH "SoLoud C++ reference checkout")

if(NOT EXISTS "${SOLOUDER_REFERENCE_DIR}/contrib/CMakeLists.txt")
  message(FATAL_ERROR
    "No SoLoud reference at ${SOLOUDER_REFERENCE_DIR}. "
    "Run: python scripts/fetch_reference.py")
endif()

set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")

# Null backend only: deterministic, no device, no audio thread.
set(SOLOUD_STATIC ON CACHE BOOL "" FORCE)
set(SOLOUD_DYNAMIC OFF CACHE BOOL "" FORCE)
set(SOLOUD_C_API OFF CACHE BOOL "" FORCE)
set(SOLOUD_BUILD_DEMOS OFF CACHE BOOL "" FORCE)
set(SOLOUD_GENERATE_GLUE OFF CACHE BOOL "" FORCE)
set(SOLOUD_BACKEND_NULL ON CACHE BOOL "" FORCE)
foreach(backend SDL2 ALSA COREAUDIO OPENSLES XAUDIO2 WINMM WASAPI)
  set(SOLOUD_BACKEND_${backend} OFF CACHE BOOL "" FORCE)
endforeach()

add_subdirectory("${SOLOUDER_REFERENCE_DIR}/contrib" soloud EXCLUDE_FROM_ALL)

# soloud_thread.cpp uses pthreads outside Windows.
find_package(Threads REQUIRED)

add_executable(reference_regressions regressions.cpp)
target_include_directories(reference_regressions PRIVATE "${SOLOUDER_REFERENCE_DIR}/include")
target_link_libraries(reference_regressions PRIVATE soloud Threads::Threads)
target_compile_definitions(reference_regressions PRIVATE
  SOLOUDER_FIXTURE_DIR="${CMAKE_CURRENT_SOURCE_DIR}/../../tests/fixtures")

# Renders tests/scenarios/*.scn for comparison with the Rust port
# (scripts/run_parity.py).
add_executable(parity_runner parity_runner.cpp)
target_include_directories(parity_runner PRIVATE "${SOLOUDER_REFERENCE_DIR}/include")
target_link_libraries(parity_runner PRIVATE soloud Threads::Threads)

# One process per test so a crash or hang fails only that test.
set(REFERENCE_TESTS
  wavstream_loop_channel_stride
  wavstream_looping_not_ended
  wavstream_truncated_data_ends
  wavstream_missing_file_ends
  inaudible_tick_over_active_cap
  protected_voice_stays_active
  protecting_a_voice_makes_it_active
  ticking_a_voice_makes_it_active
  sound_tail_survives_unaligned_mix_calls
  zero_step_voice_no_stale_scratch
  extreme_speed_makes_progress
  generic_seek_small_scratch
  negative_sample_rate_renders_silence
  seek_ignores_play_speed
  loop_point_keeps_earlier_frames
  hard_pan_is_silent_on_the_far_side
  biquad_above_voice_nyquist_stays_finite
  lofi_filters_eight_channels
  echo_with_zero_delay_survives
  waveshaper_minimum_amount_is_finite
  dc_removal_outlives_its_filter
  dc_removal_long_length_is_safe
  filter_params_reject_nan
  biquad_single_sample_stays_in_bounds
  freeverb_leaves_mono_unchanged
  freeverb_uses_its_settings
  freeverb_and_robotize_fade_parameters
  robotize_period_and_phase_are_safe
  flanger_channels_share_the_lfo
  flanger_delay_extremes_are_safe
  more_filter_params_reject_nan
  bus_seek_mixes_in_blocks
  bus_volume_channel_in_range
  bus_seek_skips_voices_without_buffers
  queue_looping_source_does_not_hang
  queue_silences_missing_channels
  lfo_from_above_to_stays_in_range
  resampled_playhead_does_not_drift
  play_stop_without_init
  play_after_failed_init
  wav_rejects_too_many_channels
  wavstream_rejects_too_many_channels
  active_voice_cap_above_255
  play3d_reused_slot_flags
  inaudible_tick_empty_loop_ends
  inaudible_tick_clears_short_block
  doppler_follows_openal
  doppler_at_sound_speed_stays_finite
  sound_speed_rejects_nan_and_infinity
  play3d_clocked_distance_delay_from_listener
  pan_ramp_leftover_frames
  surround_to_quad_center_follows_volume
  quad_to_7_1_rear_uses_rear_volume
  play_without_free_voice
  handle_slot_out_of_range
  raw_wave_rejects_bad_layout
  source_outlives_its_engine
  source_stops_on_every_engine
  source_ids_unique_across_engines
  bus_outlives_its_engine
  queue_outlives_its_engine
  bus_moved_to_another_engine
  sources_and_engines_on_two_threads
  flac_lpc_prediction_uses_64_bits
  oggstream_rewind_restarts
  oggstream_seek_lands_on_target
  oggstream_seek_past_end_ends
  oggstream_cut_first_page_fails
)

enable_testing()
foreach(test_name IN LISTS REFERENCE_TESTS)
  add_test(NAME ${test_name} COMMAND reference_regressions ${test_name})
  set_tests_properties(${test_name} PROPERTIES TIMEOUT 20)
endforeach()
