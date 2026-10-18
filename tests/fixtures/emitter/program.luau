export type Entry<T> = { value: T, label: string }

local function collect(values: {number}): {Entry<number>}
    local result = {}
    for index, value in values do
        if value > 0 then
            table.insert(result, { value = value, label = `item {index}: {value}` })
        end
    end
    return result
end

return collect({1, -2, 3})
