cmake_minimum_required(VERSION 3.20)

project(vermis_luau_oracle LANGUAGES CXX)

set(LUAU_SOURCE_DIR "" CACHE PATH "Path to the pinned Luau checkout")

if(NOT EXISTS "${LUAU_SOURCE_DIR}/CMakeLists.txt")
    message(FATAL_ERROR "LUAU_SOURCE_DIR must point to a Luau checkout")
endif()

set(LUAU_BUILD_CLI OFF CACHE BOOL "" FORCE)
set(LUAU_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(LUAU_BUILD_WEB OFF CACHE BOOL "" FORCE)
set(LUAU_BUILD_SHARED OFF CACHE BOOL "" FORCE)

add_subdirectory("${LUAU_SOURCE_DIR}" "${CMAKE_BINARY_DIR}/luau" EXCLUDE_FROM_ALL)

add_executable(vermis-luau-oracle oracle.cpp)

target_compile_features(vermis-luau-oracle PRIVATE cxx_std_17)
target_link_libraries(vermis-luau-oracle PRIVATE Luau.Ast)

set_target_properties(vermis-luau-oracle PROPERTIES RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin")

foreach(configuration Debug Release RelWithDebInfo MinSizeRel)
    string(TOUPPER "${configuration}" configuration_upper)
    set_target_properties(
        vermis-luau-oracle
        PROPERTIES "RUNTIME_OUTPUT_DIRECTORY_${configuration_upper}" "${CMAKE_BINARY_DIR}/bin"
    )
endforeach()
